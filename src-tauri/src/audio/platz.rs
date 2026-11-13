//! Ein Effekt an zwei Stellen der Kette: vor oder nach der KI-Stimme.
//!
//! Rauschunterdrückung und Gate vor einer KI-Stimme schaden ihr: Sie nehmen
//! leise Wortanfänge (h, Zischlaute) weg, die KI rät dann falsch („ände“
//! statt „Hände“). Gemessen mit Dans Wortliste ohne Filter, danach durch
//! Beatrice-Thorsten (Buchstaben-Fehler, freies Hören): ohne Filter davor
//! 37,5 %, mit Rauschunterdrückung 100 dB und Gate davor 48,1 %. Bei einer
//! KI-Figur laufen beide darum erst nach der Stimme. Es bleibt ein einziger
//! Effekt (ein Zustand, einmal geladen); er rechnet nur an der gerade
//! gültigen Stelle.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};

use super::verarbeitung::Effekt;

pub struct Platz {
    effekt: Rc<RefCell<Box<dyn Effekt>>>,
    /// Diese Stelle gilt, wenn `ki` gleich `nach_ki` ist.
    nach_ki: bool,
    ki: Arc<AtomicBool>,
}

impl Platz {
    fn gilt(&self) -> bool {
        self.ki.load(Relaxed) == self.nach_ki
    }
}

impl Effekt for Platz {
    fn verarbeite(&mut self, block: &mut [f32]) -> Result<(), String> {
        if self.gilt() { self.effekt.borrow_mut().verarbeite(block) } else { Ok(()) }
    }
    fn verzoegerung(&self) -> usize {
        if self.gilt() { self.effekt.borrow().verzoegerung() } else { 0 }
    }
}

/// (Stelle vor der Figur, Stelle nach der Figur). `ki` = gerade KI-Figur.
pub fn vor_und_nach_ki(effekt: Box<dyn Effekt>, ki: Arc<AtomicBool>) -> (Platz, Platz) {
    let effekt = Rc::new(RefCell::new(effekt));
    (Platz { effekt: effekt.clone(), nach_ki: false, ki: ki.clone() }, Platz { effekt, nach_ki: true, ki })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Zählt Blöcke und halbiert sie; meldet 7 Samples Verzögerung.
    struct Zaehler(Rc<RefCell<u32>>);
    impl Effekt for Zaehler {
        fn verarbeite(&mut self, block: &mut [f32]) -> Result<(), String> {
            *self.0.borrow_mut() += 1;
            block.iter_mut().for_each(|s| *s *= 0.5);
            Ok(())
        }
        fn verzoegerung(&self) -> usize {
            7
        }
    }

    /// Kette: vor → „Figur“ (+1) → nach. Gibt den Wert hinter der Kette zurück.
    fn durch(vor: &mut Platz, nach: &mut Platz) -> f32 {
        let mut b = vec![1.0f32; 480];
        vor.verarbeite(&mut b).unwrap();
        b.iter_mut().for_each(|s| *s += 1.0);
        nach.verarbeite(&mut b).unwrap();
        b[0]
    }

    #[test]
    fn rechnet_nur_an_der_gueltigen_stelle() {
        let zahl = Rc::new(RefCell::new(0));
        let ki = Arc::new(AtomicBool::new(false));
        let (mut vor, mut nach) = vor_und_nach_ki(Box::new(Zaehler(zahl.clone())), ki.clone());
        // Keine KI: erst halbieren, dann +1.
        assert_eq!(durch(&mut vor, &mut nach), 1.5);
        assert_eq!((vor.verzoegerung(), nach.verzoegerung()), (7, 0));
        // KI: erst +1, dann halbieren.
        ki.store(true, Relaxed);
        assert_eq!(durch(&mut vor, &mut nach), 1.0);
        assert_eq!((vor.verzoegerung(), nach.verzoegerung()), (0, 7));
        // Ein Effekt, je Block genau einmal gerechnet.
        assert_eq!(*zahl.borrow(), 2);
    }
}
