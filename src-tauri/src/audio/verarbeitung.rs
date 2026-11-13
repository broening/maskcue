//! Verarbeitungs-Faden: rohes Mikrofon → gemeinsame Effekte (Denoise, …)
//! → je Ausgang eigene Effekte (Figur-Stimme, …) → Ausgangs-Puffer.
//!
//! Läuft getrennt vom Mikrofon-Faden. So darf ein Effekt Speicher anlegen
//! (DeepFilterNet tut das), ohne den Mikrofon-Strom zu stören. Der
//! Mikrofon-Faden weckt diesen Faden nach jedem Paket.

use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use windows::Win32::System::Threading::AvSetMmThreadCharacteristicsW;
use windows::core::w;

use super::denoise::{BLOCK, Denoise};
use super::pipe::{INTERN_RATE, InputSide, RohLeser, Stats};

/// Ein Effekt in der Kette. Arbeitet auf Blöcken von [`BLOCK`] Samples (48 kHz, mono).
/// Muss nicht `Send` sein: Effekte entstehen im Verarbeitungs-Faden selbst
/// (DeepFilterNet darf den Faden nicht wechseln).
pub trait Effekt {
    fn verarbeite(&mut self, block: &mut [f32]) -> Result<(), String>;
    /// Verzögerung, die der Effekt hinzufügt (Samples).
    fn verzoegerung(&self) -> usize;
}

impl Effekt for Denoise {
    fn verarbeite(&mut self, block: &mut [f32]) -> Result<(), String> {
        Denoise::verarbeite(self, block).map(|_| ())
    }
    fn verzoegerung(&self) -> usize {
        Denoise::verzoegerung(self)
    }
}

/// Effekte für alle Ausgänge und je Ausgang.
#[derive(Default)]
pub struct Kette {
    /// Laufen einmal, vor der Aufteilung (Denoise, Gate, Kompressor, EQ).
    pub gemeinsam: Vec<Box<dyn Effekt>>,
    /// Je Ausgang, in derselben Reihenfolge wie die Ausgänge. Fehlt ein
    /// Eintrag, bekommt der Ausgang keine eigenen Effekte.
    pub pro_ausgang: Vec<Vec<Box<dyn Effekt>>>,
}

/// Baut die Effekt-Kette. Läuft im Verarbeitungs-Faden.
pub type KettenBau = Box<dyn FnOnce() -> Result<Kette, String> + Send>;

/// Ein Ziel der Verarbeitung: Puffer eines Ausgangs und seine Messwerte.
pub struct Ziel {
    pub puffer: InputSide,
    pub stats: Arc<Stats>,
}

/// Die ersten Blöcke nach dem Start kommen unregelmäßig. In dieser Zeit
/// zählen nur normale Lücken (bis [`ANLAUF_MAX_LUECKE_US`]), keine Start-Störungen.
const ANLAUF_BLOECKE: u32 = 300;
const ANLAUF_MAX_LUECKE_US: u64 = 35_000;

/// Läuft, solange der Wert lebt.
pub struct Verarbeitung {
    stop: Arc<AtomicBool>,
    faden: Option<JoinHandle<()>>,
    /// Verzögerung aller Effekte je Ausgang beim Start (Samples).
    pub verzoegerung: Vec<usize>,
}

impl Verarbeitung {
    pub fn faden(&self) -> std::thread::Thread {
        self.faden.as_ref().expect("Faden läuft").thread().clone()
    }
}

impl Drop for Verarbeitung {
    fn drop(&mut self) {
        self.stop.store(true, Relaxed);
        if let Some(f) = self.faden.take() {
            f.thread().unpark();
            let _ = f.join();
        }
    }
}

fn summe(kette: &[Box<dyn Effekt>]) -> usize {
    kette.iter().map(|e| e.verzoegerung()).sum()
}

fn anwenden(kette: &mut [Box<dyn Effekt>], block: &mut [f32], stats: &Stats) {
    for effekt in kette.iter_mut() {
        if let Err(e) = effekt.verarbeite(block) {
            stats.fehler_merken(e);
        }
    }
}

/// Startet den Faden. `roh`: Mikrofon (48 kHz, mono). `ziele`: ein Eintrag je Ausgang.
/// `stats`: Messwerte des Eingangs (Fehler der gemeinsamen Effekte landen hier).
pub fn start(mut roh: RohLeser, mut ziele: Vec<Ziel>, bau: KettenBau, stats: Arc<Stats>) -> Result<Verarbeitung, String> {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_faden = stop.clone();
    let (tx, rx) = mpsc::channel();
    let faden = std::thread::Builder::new()
        .name("maskcue-verarbeitung".into())
        .spawn(move || {
            let mut kette = match bau() {
                Ok(k) => k,
                Err(e) => {
                    let _ = tx.send(Err(e));
                    return;
                }
            };
            kette.pro_ausgang.resize_with(ziele.len(), Vec::new);
            let gemeinsam = summe(&kette.gemeinsam);
            let verzoegerung: Vec<usize> = kette.pro_ausgang.iter().map(|k| gemeinsam + summe(k)).collect();
            let _ = tx.send(Ok(verzoegerung));

            let mut task = 0u32;
            let _ = unsafe { AvSetMmThreadCharacteristicsW(w!("Pro Audio"), &mut task) };
            let mut block = [0.0f32; BLOCK];
            let mut kopie = [0.0f32; BLOCK];
            let mut letzter_push: Option<Instant> = None;
            let mut anlauf = ANLAUF_BLOECKE;
            while !stop_faden.load(Relaxed) {
                while roh.verfuegbar() >= BLOCK {
                    let Some(aufnahme) = roh.lies_block(&mut block) else { break };
                    anwenden(&mut kette.gemeinsam, &mut block, &stats);

                    // Lücke zwischen zwei fertigen Blöcken: Mikrofon-Lücken plus Rechenzeit.
                    let jetzt = Instant::now();
                    let luecke = letzter_push.map(|v| jetzt.duration_since(v).as_micros() as u64);
                    letzter_push = Some(jetzt);
                    if anlauf > 0 {
                        anlauf -= 1;
                    }

                    for (i, ziel) in ziele.iter_mut().enumerate() {
                        kopie.copy_from_slice(&block);
                        anwenden(&mut kette.pro_ausgang[i], &mut kopie, &ziel.stats);
                        if let Some(l) = luecke {
                            if anlauf == 0 || l <= ANLAUF_MAX_LUECKE_US {
                                ziel.stats.ein_luecke_merken(l);
                            }
                        }
                        // Aufnahmezeit des Samples, das jetzt herauskommt: um die Verzögerung früher.
                        // Jeden Block neu: „Figur aus“ ändert die Verzögerung im Lauf.
                        let v = gemeinsam + summe(&kette.pro_ausgang[i]);
                        let v_ns = (v as f64 * 1e9 / INTERN_RATE as f64) as u64;
                        ziel.puffer.push(&kopie, 1, aufnahme.saturating_sub(v_ns), |s| s);
                    }
                }
                std::thread::park_timeout(Duration::from_millis(20));
            }
        })
        .map_err(|e| e.to_string())?;
    match rx.recv() {
        Ok(Ok(verzoegerung)) => Ok(Verarbeitung { stop, faden: Some(faden), verzoegerung }),
        Ok(Err(e)) => {
            let _ = faden.join();
            Err(e)
        }
        Err(_) => Err("Verarbeitung: Faden ohne Antwort beendet".into()),
    }
}
