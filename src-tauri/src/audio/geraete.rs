//! Audio-Geräte finden (WASAPI über cpal).

use cpal::Device;
use cpal::traits::HostTrait;

/// Namen aller Eingänge und Ausgänge.
pub fn liste() -> Result<(Vec<String>, Vec<String>), String> {
    let host = cpal::default_host();
    let namen = |it: Box<dyn Iterator<Item = Device>>| it.map(|d| d.to_string()).collect::<Vec<_>>();
    let ein = host.input_devices().map_err(|e| e.to_string())?;
    let aus = host.output_devices().map_err(|e| e.to_string())?;
    Ok((namen(Box::new(ein)), namen(Box::new(aus))))
}

pub fn eingang(name: &str) -> Result<Device, String> {
    let host = cpal::default_host();
    let mut geraete: Vec<Device> = host.input_devices().map_err(|e| e.to_string())?.collect();
    let namen: Vec<String> = geraete.iter().map(|d| d.to_string()).collect();
    let i = waehle(&namen, name, "Eingang")?;
    Ok(geraete.swap_remove(i))
}

/// Erst genauer Name, dann Teil des Namens (ohne Groß/klein).
/// Passen mehrere Geräte auf einen Teil-Namen, ist das ein Fehler.
pub fn waehle(namen: &[String], name: &str, art: &str) -> Result<usize, String> {
    if let Some(i) = namen.iter().position(|n| n == name) {
        return Ok(i);
    }
    let klein = name.to_lowercase();
    // Name genau vor der Klammer, z. B. "MaskCue" für "MaskCue (Elgato Virtual Audio)".
    // So ist "MaskCue" eindeutig, obwohl es auch in "MaskCue Chat (…)" steckt.
    let vor_klammer = format!("{klein} (");
    let genau: Vec<usize> = (0..namen.len()).filter(|&i| namen[i].to_lowercase().starts_with(&vor_klammer)).collect();
    if let [i] = genau.as_slice() {
        return Ok(*i);
    }
    let treffer: Vec<usize> = (0..namen.len()).filter(|&i| namen[i].to_lowercase().contains(&klein)).collect();
    match treffer.as_slice() {
        [i] => Ok(*i),
        [] => Err(format!("{art} \"{name}\" nicht gefunden")),
        _ => Err(format!(
            "{art} \"{name}\" ist nicht eindeutig: {}",
            treffer.iter().map(|&i| format!("\"{}\"", namen[i])).collect::<Vec<_>>().join(", ")
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::waehle;

    fn namen() -> Vec<String> {
        ["MaskCue Chat (Elgato Virtual Audio)", "Game (Elgato Virtual Audio)", "MaskCue (Elgato Virtual Audio)", "Tischmikrofon (2- USB Audio Device)"]
            .map(String::from)
            .to_vec()
    }

    #[test]
    fn name_vor_klammer_gewinnt_gegen_teiltreffer() {
        assert_eq!(waehle(&namen(), "MaskCue", "Ausgang"), Ok(2));
        assert_eq!(waehle(&namen(), "maskcue chat", "Ausgang"), Ok(0));
    }

    #[test]
    fn teilname_bleibt_moeglich() {
        assert_eq!(waehle(&namen(), "Tischmikro", "Eingang"), Ok(3));
        assert_eq!(waehle(&namen(), "MaskCue Chat (Elgato Virtual Audio)", "Ausgang"), Ok(0));
        assert!(waehle(&namen(), "G", "Ausgang").is_err());
    }
}
