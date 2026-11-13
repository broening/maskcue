// C-Brücke zu Signalsmith Stretch (MIT, https://github.com/Signalsmith-Audio/signalsmith-stretch).
// Nur Mono, nur was MaskCue braucht. Rust ruft diese Funktionen über FFI auf
// (src/audio/stimme.rs). Kein bindgen nötig, darum kein LLVM beim Bauen.

#include "signalsmith-stretch/signalsmith-stretch.h"

using Stretch = signalsmith::stretch::SignalsmithStretch<float>;

extern "C" {

void *gx_stretch_neu(int block, int intervall, int verteilen) {
    // Fester Startwert für den Zufall: gleiches Ergebnis bei gleichem Eingang (Tests).
    auto *s = new Stretch(12345);
    s->configure(1, block, intervall, verteilen != 0);
    return s;
}

void gx_stretch_loeschen(void *s) { delete static_cast<Stretch *>(s); }

void gx_stretch_zuruecksetzen(void *s) { static_cast<Stretch *>(s)->reset(); }

int gx_stretch_latenz(void *s) {
    auto *st = static_cast<Stretch *>(s);
    return st->inputLatency() + st->outputLatency();
}

void gx_stretch_tonhoehe(void *s, float faktor, float tonal_grenze) {
    static_cast<Stretch *>(s)->setTransposeFactor(faktor, tonal_grenze);
}

void gx_stretch_formant(void *s, float faktor, int tonhoehe_ausgleichen) {
    static_cast<Stretch *>(s)->setFormantFactor(faktor, tonhoehe_ausgleichen != 0);
}

void gx_stretch_formant_basis(void *s, float hz) { static_cast<Stretch *>(s)->setFormantBase(hz); }

void gx_stretch_verarbeite(void *s, const float *ein, float *aus, int laenge) {
    const float *eingaenge[1] = {ein};
    float *ausgaenge[1] = {aus};
    static_cast<Stretch *>(s)->process(eingaenge, laenge, ausgaenge, laenge);
}

}
