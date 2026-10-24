//! Le micro : un flux ouvert en continu (cpal, WASAPI), mis en mono, et deux façons d'en tirer une phrase.
//!
//! - **La touche** : ce qui se dit pendant qu'elle est tenue — plus une demi-seconde d'avance, pour que le début d'une
//!   phrase ne soit jamais coupé (un casque Bluetooth met du temps à passer en mode appel).
//! - **L'écoute continue** (le mot d'éveil « Eve ») : un détecteur de parole découpe les phrases au silence ; le seuil
//!   suit le bruit de fond du bureau. Chaque phrase attend dans une file que le chef la donne à l'oreille, qui ne garde
//!   que celles qui commencent par « Eve ». En sourdine pendant qu'Eve parle : elle ne s'entend pas elle-même.
//!
//! Tout se ramène à 16 kHz, la cadence de Whisper.

use std::collections::VecDeque;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, SizedSample};

/// La cadence de Whisper.
pub const CADENCE: u32 = 16_000;
/// Le seuil de parole le plus bas (moyenne quadratique), quel que soit le silence du bureau.
pub const SEUIL_MIN: f32 = 0.012;

/// Le détecteur de parole de l'écoute continue, par blocs de 20 ms.
#[derive(Default)]
struct Detecteur {
    en_parole: bool,
    phrase: Vec<f32>,
    bloc: Vec<f32>,
    silence: usize,
    au_dessus: usize,
    bruit: f32,
}

impl Detecteur {
    fn oublier(&mut self) {
        self.en_parole = false;
        self.phrase.clear();
        self.bloc.clear();
        self.silence = 0;
        self.au_dessus = 0;
    }

    /// Un échantillon de plus ; une phrase finie (0,7 s de silence, ou 20 s de parole) part dans `phrases`.
    fn ajouter(&mut self, m: f32, taux: u32, avance: &VecDeque<f32>, phrases: &mut VecDeque<Vec<f32>>) {
        self.bloc.push(m);
        if self.bloc.len() < (taux / 50).max(1) as usize {
            return;
        }
        let niveau = rms(&self.bloc);
        let seuil = (self.bruit * 3.0).max(SEUIL_MIN);
        if !self.en_parole {
            // Le bruit de fond suit lentement le silence (sans se laisser emporter par un éclat).
            self.bruit = if self.bruit == 0.0 {
                niveau
            } else {
                self.bruit * 0.97 + niveau.min(self.bruit * 2.0 + 0.001) * 0.03
            };
            self.au_dessus = if niveau > seuil { self.au_dessus + 1 } else { 0 };
            if self.au_dessus >= 3 {
                // 60 ms au-dessus du seuil : une phrase commence, avec sa demi-seconde d'avance.
                self.en_parole = true;
                self.silence = 0;
                self.phrase = avance.iter().copied().collect();
            }
        } else {
            self.phrase.extend_from_slice(&self.bloc);
            self.silence = if niveau < seuil * 0.7 { self.silence + self.bloc.len() } else { 0 };
            let fin = self.silence > taux as usize * 7 / 10 || self.phrase.len() > taux as usize * 20;
            if fin {
                let utile = self.phrase.len().saturating_sub(self.silence);
                if utile > taux as usize * 4 / 10 {
                    phrases.push_back(std::mem::take(&mut self.phrase));
                    while phrases.len() > 4 {
                        phrases.pop_front();
                    }
                }
                self.oublier();
            }
        }
        self.bloc.clear();
    }
}

struct Capture {
    taux: u32,
    enregistre: bool,
    tampon: Vec<f32>,
    avance: VecDeque<f32>,
    avance_max: usize,
    niveau: f32,
    en_panne: bool,
    continue_: bool,
    sourdine: bool,
    detecteur: Detecteur,
    phrases: VecDeque<Vec<f32>>,
    /// Les accrocs du flux (`Xrun`) depuis son ouverture : comptés, notés au journal de loin en loin ([`signaler`]).
    accrocs: u64,
    /// L'arrivée du dernier paquet de son : un flux qui n'en livre plus est mort, même sans erreur ([`en_panne_de`]).
    dernier_paquet: Instant,
}

impl Capture {
    fn nouvelle(taux: u32) -> Capture {
        Capture {
            taux,
            enregistre: false,
            tampon: Vec::new(),
            avance: VecDeque::new(),
            avance_max: taux as usize / 2,
            niveau: 0.0,
            en_panne: false,
            continue_: false,
            sourdine: false,
            detecteur: Detecteur::default(),
            phrases: VecDeque::new(),
            accrocs: 0,
            dernier_paquet: Instant::now(),
        }
    }
}

pub struct Micro {
    _flux: cpal::Stream,
    partage: Arc<Mutex<Capture>>,
    taux: u32,
    pub nom: String,
}

impl Micro {
    /// Le micro par défaut de Windows.
    pub fn ouvrir() -> Result<Micro, String> {
        let hote = cpal::default_host();
        let appareil = hote.default_input_device().ok_or("aucun micro branché")?;
        let nom = appareil.description().map(|d| d.name().to_string()).unwrap_or_else(|_| "micro".into());
        let cfg = appareil.default_input_config().map_err(|e| format!("le micro ne dit pas son format : {e}"))?;
        let canaux = usize::from(cfg.channels()).max(1);
        let taux = cfg.sample_rate();
        let partage = Arc::new(Mutex::new(Capture::nouvelle(taux)));
        let config = cfg.config();
        let flux = match cfg.sample_format() {
            SampleFormat::F32 => construire::<f32>(&appareil, config, canaux, &partage, de_f32),
            SampleFormat::I16 => construire::<i16>(&appareil, config, canaux, &partage, de_i16),
            SampleFormat::U16 => construire::<u16>(&appareil, config, canaux, &partage, de_u16),
            SampleFormat::I32 => construire::<i32>(&appareil, config, canaux, &partage, de_i32),
            autre => return Err(format!("format du micro non pris en charge : {autre:?}")),
        }?;
        flux.play().map_err(|e| format!("le micro ne démarre pas : {e}"))?;
        Ok(Micro { _flux: flux, partage, taux, nom })
    }

    /// La touche est enfoncée : on garde tout, à partir de la demi-seconde d'avance ; l'écoute continue s'efface.
    pub fn debut(&self) {
        if let Ok(mut c) = self.partage.lock() {
            let avance: Vec<f32> = c.avance.drain(..).collect();
            c.tampon = avance;
            c.enregistre = true;
            c.detecteur.oublier();
        }
    }

    /// La touche est relâchée : ce qui a été dit, à 16 kHz.
    pub fn fin(&self) -> Vec<f32> {
        let brut = self
            .partage
            .lock()
            .map(|mut c| {
                c.enregistre = false;
                std::mem::take(&mut c.tampon)
            })
            .unwrap_or_default();
        vers_16k(&brut, self.taux)
    }

    /// L'écoute continue (le mot d'éveil), en marche ou non.
    pub fn ecoute_continue(&self, oui: bool) {
        if let Ok(mut c) = self.partage.lock() {
            c.continue_ = oui;
            if !oui {
                c.detecteur.oublier();
                c.phrases.clear();
            }
        }
    }

    /// En sourdine pendant qu'Eve parle : la phrase en cours s'oublie, rien de neuf ne commence.
    pub fn sourdine(&self, oui: bool) {
        if let Ok(mut c) = self.partage.lock() {
            if oui && !c.sourdine {
                c.detecteur.oublier();
            }
            c.sourdine = oui;
        }
    }

    /// La prochaine phrase de l'écoute continue, à 16 kHz.
    pub fn prendre_phrase(&self) -> Option<Vec<f32>> {
        let p = self.partage.lock().ok()?.phrases.pop_front()?;
        Some(vers_16k(&p, self.taux))
    }

    /// Une phrase est-elle en train de se dire (le détecteur de l'écoute continue l'entend) ?
    pub fn en_parole(&self) -> bool {
        self.partage.lock().map(|c| c.detecteur.en_parole).unwrap_or(false)
    }

    /// Le niveau du son (moyenne quadratique, lissée).
    pub fn niveau(&self) -> f32 {
        self.partage.lock().map(|c| c.niveau).unwrap_or(0.0)
    }

    /// Le flux est-il en panne : une erreur qui l'arrête (micro débranché), ou plus aucun son depuis un moment ?
    pub fn en_panne(&self) -> bool {
        self.partage.lock().map(|c| en_panne_de(&c, Instant::now())).unwrap_or(true)
    }
}

/// Un flux sans paquet depuis ce temps est mort : WASAPI en livre un toutes les 10 ms environ, silence compris.
const SILENCE_MAX: Duration = Duration::from_secs(2);

/// Un flux qui se tait sans erreur — l'appareil pris en mode exclusif par un autre programme, son format changé, le
/// service audio redémarré : cpal ne réveille pas toujours sa boucle pour le dire (`host/wasapi/stream.rs`). Avant,
/// la réouverture toutes les 5 s le réparait par accident ; sans elle, Eve resterait sourde, « Prête » à l'écran.
fn en_panne_de(c: &Capture, maintenant: Instant) -> bool {
    c.en_panne || maintenant.saturating_duration_since(c.dernier_paquet) > SILENCE_MAX
}

fn de_f32(x: f32) -> f32 {
    x
}
fn de_i16(x: i16) -> f32 {
    f32::from(x) / 32_768.0
}
fn de_u16(x: u16) -> f32 {
    (f32::from(x) - 32_768.0) / 32_768.0
}
fn de_i32(x: i32) -> f32 {
    x as f32 / 2_147_483_648.0
}

fn construire<T>(
    appareil: &cpal::Device,
    config: cpal::StreamConfig,
    canaux: usize,
    partage: &Arc<Mutex<Capture>>,
    conv: fn(T) -> f32,
) -> Result<cpal::Stream, String>
where
    T: SizedSample + Copy + Send + 'static,
{
    let p = partage.clone();
    let p_erreur = partage.clone();
    appareil
        .build_input_stream::<T, _, _>(
            config,
            move |donnees: &[T], _: &cpal::InputCallbackInfo| recevoir(&p, donnees, canaux, conv),
            move |e: cpal::Error| {
                // Le journal s'écrit hors du verrou : le fil du son ne l'attend pas.
                if let Some(ligne) = p_erreur.lock().ok().and_then(|mut c| signaler(&mut c, &e)) {
                    crate::journal::ecrire(&ligne);
                }
            },
            None,
        )
        .map_err(|e| format!("le micro ne s'ouvre pas : {e}"))
}

/// Une erreur du flux qui demande de rouvrir le micro. Deux n'en sont pas, et cpal continue de livrer le son après
/// elles : un accroc (`Xrun`, un tampon débordé ou vide — sous WASAPI, le seul que cpal 0.18.2 émette ainsi,
/// `host/wasapi/stream.rs`) et une priorité temps réel refusée (`RealtimeDenied`, qu'Eve ne voit pas : la fonction
/// `realtime` de cpal n'est pas compilée). Compter l'accroc pour une panne rouvrait le micro toutes les 5 s, sans fin :
/// une webcam LifeCam VX-5500 en signale presque à chaque ouverture, et parfois des dizaines par flux ; le journal d'une
/// Eve au même code en a compté 11 904 en moins de 13 h (le 2026-10-22), et une réouverture en pleine phrase la perdait.
fn panne(e: &cpal::Error) -> bool {
    !matches!(e.kind(), cpal::ErrorKind::Xrun | cpal::ErrorKind::RealtimeDenied)
}

/// Une erreur du flux : une panne marque le micro (le chef le rouvrira) ; un accroc se compte, sans rien rouvrir. Rend
/// la ligne du journal, s'il y en a une : chaque panne, et, des accrocs, le 1er, le 10e, le 100e… — la mesure d'un son
/// haché reste au journal sans le noyer. Un accroc n'est que le drapeau de discontinuité de WASAPI : la LifeCam en
/// signale 10 par minute sans perdre un seul échantillon (`eve --essai-micro`, le 2026-10-23) ; un autre micro peut en
/// perdre, l'essai le dit.
fn signaler(c: &mut Capture, e: &cpal::Error) -> Option<String> {
    if panne(e) {
        c.en_panne = true;
        return Some(format!("micro : {e}"));
    }
    c.accrocs += 1;
    let n = c.accrocs;
    puissance_de_dix(n).then(|| {
        format!(
            "micro : accroc signalé par Windows ({n} depuis l'ouverture ; le flux continue — eve --essai-micro dit s'il \
             perd du son)"
        )
    })
}

fn puissance_de_dix(mut n: u64) -> bool {
    while n >= 10 && n.is_multiple_of(10) {
        n /= 10;
    }
    n == 1
}

fn recevoir<T: Copy>(partage: &Mutex<Capture>, donnees: &[T], canaux: usize, conv: fn(T) -> f32) {
    let Ok(mut c) = partage.lock() else { return };
    let c = &mut *c;
    c.dernier_paquet = Instant::now();
    let mut somme = 0.0f32;
    let mut n = 0usize;
    let ecoute = c.continue_;
    for trame in donnees.chunks(canaux) {
        let m = trame.iter().map(|&x| conv(x)).sum::<f32>() / trame.len() as f32;
        somme += m * m;
        n += 1;
        if c.enregistre {
            c.tampon.push(m);
        } else if c.sourdine {
            // La voix d'Eve n'entre pas dans l'avance : une touche enfoncée pendant qu'elle parle ne lui prête pas ses
            // mots.
            c.avance.clear();
        } else {
            c.avance.push_back(m);
            if c.avance.len() > c.avance_max {
                c.avance.pop_front();
            }
            if ecoute {
                c.detecteur.ajouter(m, c.taux, &c.avance, &mut c.phrases);
            }
        }
    }
    if n > 0 {
        let rms = (somme / n as f32).sqrt();
        c.niveau = c.niveau * 0.5 + rms * 0.5;
    }
}

/// Ramène un son mono à 16 kHz : une moyenne glissante (un passe-bas sommaire, contre le repliement), puis une
/// interpolation linéaire. Whisper n'en demande pas plus pour de la parole.
pub fn vers_16k(x: &[f32], taux: u32) -> Vec<f32> {
    if taux == CADENCE || x.is_empty() || taux == 0 {
        return x.to_vec();
    }
    let rapport = f64::from(taux) / f64::from(CADENCE);
    let k = rapport.round().max(1.0) as usize;
    let filtre: Vec<f32> = if k > 1 {
        let mut s = 0.0f32;
        x.iter()
            .enumerate()
            .map(|(i, &v)| {
                s += v;
                if i >= k {
                    s -= x[i - k];
                }
                s / (i.min(k - 1) + 1) as f32
            })
            .collect()
    } else {
        x.to_vec()
    };
    let n = (x.len() as f64 / rapport).floor() as usize;
    (0..n)
        .map(|i| {
            let pos = i as f64 * rapport;
            let j = pos.floor() as usize;
            let f = (pos - j as f64) as f32;
            let a = filtre[j.min(filtre.len() - 1)];
            let b = *filtre.get(j + 1).unwrap_or(&a);
            a + (b - a) * f
        })
        .collect()
}

/// La moyenne quadratique d'un son : sous 0,0025, c'est un silence.
pub fn rms(x: &[f32]) -> f32 {
    if x.is_empty() {
        return 0.0;
    }
    (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
}

/// Un fichier WAV (PCM 16 bits, toutes cadences, mono ou stéréo), à 16 kHz mono : pour les essais.
pub fn lire_wav(chemin: &Path) -> Result<Vec<f32>, String> {
    let o = std::fs::read(chemin).map_err(|e| format!("{} : {e}", chemin.display()))?;
    if o.len() < 12 || &o[0..4] != b"RIFF" || &o[8..12] != b"WAVE" {
        return Err("ce n'est pas un fichier WAV".into());
    }
    let (mut canaux, mut taux, mut bits, mut format) = (1u16, CADENCE, 16u16, 1u16);
    let mut i = 12;
    while i + 8 <= o.len() {
        let id = &o[i..i + 4];
        let taille = u32::from_le_bytes([o[i + 4], o[i + 5], o[i + 6], o[i + 7]]) as usize;
        let d = i + 8;
        if id == b"fmt " && taille >= 16 && d + 16 <= o.len() {
            format = u16::from_le_bytes([o[d], o[d + 1]]);
            canaux = u16::from_le_bytes([o[d + 2], o[d + 3]]);
            taux = u32::from_le_bytes([o[d + 4], o[d + 5], o[d + 6], o[d + 7]]);
            bits = u16::from_le_bytes([o[d + 14], o[d + 15]]);
        } else if id == b"data" {
            if bits != 16 || (format != 1 && format != 0xFFFE) {
                return Err(format!("WAV non pris en charge (format {format}, {bits} bits)"));
            }
            let fin = (d + taille).min(o.len());
            let echantillons: Vec<f32> = o[d..fin]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| f32::from(i16::from_le_bytes([b[0], b[1]])) / 32_768.0)
                .collect();
            let c = usize::from(canaux.max(1));
            let mono: Vec<f32> = echantillons.chunks(c).map(|t| t.iter().sum::<f32>() / t.len() as f32).collect();
            return Ok(vers_16k(&mono, taux));
        }
        i = d + taille + (taille & 1);
    }
    Err("pas de données dans le WAV".into())
}

#[cfg(test)]
mod essais {
    use super::*;

    #[test]
    fn la_cadence_se_ramene_a_16_khz() {
        let x: Vec<f32> = (0..48_000).map(|i| (i as f32 * 0.01).sin()).collect();
        assert_eq!(vers_16k(&x, 48_000).len(), 16_000);
        assert_eq!(vers_16k(&x, 16_000).len(), 48_000);
        let y: Vec<f32> = vec![0.0; 44_100];
        assert_eq!(vers_16k(&y, 44_100).len(), 16_000);
    }

    /// Un accroc ne rouvre pas le micro (il le rouvrait toutes les 5 s, sans fin) ; toute autre sorte d'erreur de cpal
    /// 0.18.2, si — la liste est complète : une exemption de trop (`HostUnavailable`, le service audio arrêté) rendrait
    /// Eve sourde.
    #[test]
    fn un_accroc_n_est_pas_une_panne() {
        use cpal::ErrorKind as K;
        assert!(!panne(&cpal::Error::new(K::Xrun)));
        assert!(!panne(&cpal::Error::new(K::RealtimeDenied)));
        for k in [
            K::DeviceBusy,
            K::DeviceChanged,
            K::DeviceNotAvailable,
            K::HostUnavailable,
            K::InvalidInput,
            K::PermissionDenied,
            K::ResourceExhausted,
            K::StreamInvalidated,
            K::UnsupportedConfig,
            K::UnsupportedOperation,
            K::BackendError,
            K::Other,
        ] {
            assert!(panne(&cpal::Error::new(k)), "{k:?}");
        }
    }

    /// Le rappel d'erreur du flux : un accroc se compte sans marquer le micro, et ne se note qu'au 1er, 10e, 100e… ;
    /// une panne le marque, et se note toujours.
    #[test]
    fn un_accroc_se_compte_sans_rouvrir_et_une_panne_marque_le_micro() {
        let mut c = Capture::nouvelle(16_000);
        let accroc = cpal::Error::new(cpal::ErrorKind::Xrun);
        let mut notes = Vec::new();
        for n in 1..=1000u64 {
            if signaler(&mut c, &accroc).is_some() {
                notes.push(n);
            }
        }
        assert_eq!(notes, vec![1, 10, 100, 1000]);
        assert_eq!(c.accrocs, 1000);
        assert!(!c.en_panne, "un accroc ne rouvre pas le micro");
        assert!(!en_panne_de(&c, c.dernier_paquet));
        assert!(signaler(&mut c, &cpal::Error::new(cpal::ErrorKind::DeviceNotAvailable)).is_some());
        assert!(c.en_panne && en_panne_de(&c, c.dernier_paquet));
    }

    /// Un flux qui se tait sans erreur est en panne passé deux secondes ; chaque paquet reçu le remet à zéro.
    #[test]
    fn un_flux_qui_ne_livre_plus_rien_est_en_panne() {
        let partage = Mutex::new(Capture::nouvelle(16_000));
        let ouvert = partage.lock().unwrap().dernier_paquet;
        assert!(!en_panne_de(&partage.lock().unwrap(), ouvert + Duration::from_millis(1_900)));
        assert!(en_panne_de(&partage.lock().unwrap(), ouvert + SILENCE_MAX + Duration::from_millis(1)));
        std::thread::sleep(Duration::from_millis(20));
        recevoir(&partage, &[0.0f32; 160], 1, de_f32);
        let c = partage.lock().unwrap();
        assert!(c.dernier_paquet > ouvert, "un paquet reçu remet la montre à zéro");
        assert!(!en_panne_de(&c, ouvert + SILENCE_MAX + Duration::from_millis(1)));
    }

    #[test]
    fn un_silence_a_un_niveau_nul() {
        assert_eq!(rms(&[]), 0.0);
        assert!(rms(&[0.0; 100]) < 0.0025);
        assert!(rms(&[0.5, -0.5, 0.5, -0.5]) > 0.4);
    }

    /// Une seconde de silence, une seconde de « parole » (un son fort), une seconde de silence : une phrase.
    #[test]
    fn le_detecteur_decoupe_une_phrase() {
        let taux = 16_000;
        let mut d = Detecteur::default();
        let mut avance = VecDeque::new();
        let mut phrases = VecDeque::new();
        let son: Vec<f32> = (0..taux * 3)
            .map(|i| if (taux..taux * 2).contains(&i) { 0.3 * ((i as f32) * 0.05).sin() } else { 0.001 })
            .collect();
        for &m in &son {
            avance.push_back(m);
            if avance.len() > taux as usize / 2 {
                avance.pop_front();
            }
            d.ajouter(m, taux, &avance, &mut phrases);
        }
        assert_eq!(phrases.len(), 1);
        let p = &phrases[0];
        assert!(p.len() > taux as usize && p.len() < taux as usize * 2 + taux as usize / 2, "{}", p.len());
    }
}
