//! Le micro : un flux ouvert en continu (cpal, WASAPI), mis en mono, et deux façons d'en tirer une phrase.
//!
//! - **La touche** : ce qui se dit pendant qu'elle est tenue — plus une demi-seconde d'avance, pour que le début d'une
//!   phrase ne soit jamais coupé (un casque Bluetooth met du temps à passer en mode appel).
//! - **L'écoute continue** (le mot d'éveil « Eve ») : un détecteur de parole découpe les phrases au silence ; le seuil
//!   suit le bruit de fond du bureau. Chaque phrase attend dans une file que le chef la donne à l'oreille, qui ne garde
//!   que celles qui commencent par « Eve ». En sourdine pendant qu'Eve parle : elle ne s'entend pas elle-même.
//!
//! Tout se ramène à 16 kHz, la cadence de Whisper.
//!
//! La poignée d'essai `EVE_MICRO=<dossier>` remplace le micro du poste par les sons qu'on dépose dans ce dossier
//! ([`Micro::ouvrir`]) : le tour de parole s'essaie de bout en bout, sans personne devant le poste ni rien qui joue
//! dans la pièce.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, SizedSample};

/// La cadence de Whisper.
pub const CADENCE: u32 = 16_000;
/// Le seuil de parole le plus bas (moyenne quadratique), quel que soit le silence du bureau.
pub const SEUIL_MIN: f32 = 0.012;

/// Le silence qui finit une phrase de l'écoute continue : la phrase part alors à l'oreille. Une pause plus longue au
/// milieu d'une phrase la coupe ; la fin du tour (`parole::FinDeTour`) en attend la suite quand elle s'arrête en
/// suspens.
pub const SILENCE_DE_FIN: f32 = 0.7;

/// Le détecteur de parole de l'écoute continue, par blocs de 20 ms.
struct Detecteur {
    en_parole: bool,
    phrase: Vec<f32>,
    bloc: Vec<f32>,
    silence: usize,
    au_dessus: usize,
    bruit: f32,
    /// Le silence qui finit une phrase, en secondes ([`SILENCE_DE_FIN`] ; un autre pour l'essai du tour).
    fin: f32,
}

impl Default for Detecteur {
    fn default() -> Detecteur {
        Detecteur {
            en_parole: false,
            phrase: Vec::new(),
            bloc: Vec::new(),
            silence: 0,
            au_dessus: 0,
            bruit: 0.0,
            fin: SILENCE_DE_FIN,
        }
    }
}

impl Detecteur {
    fn oublier(&mut self) {
        self.en_parole = false;
        self.phrase.clear();
        self.bloc.clear();
        self.silence = 0;
        self.au_dessus = 0;
    }

    /// Un échantillon de plus ; une phrase finie (son silence de fin, ou 20 s de parole) part dans `phrases`.
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
            let fin =
                self.silence > (taux as f32 * self.fin).round() as usize || self.phrase.len() > taux as usize * 20;
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
    /// Le flux du micro du poste ; aucun pour le micro d'essai ([`micro_d_essai`]).
    _flux: Option<cpal::Stream>,
    partage: Arc<Mutex<Capture>>,
    taux: u32,
    pub nom: String,
}

impl Micro {
    /// Le micro par défaut de Windows ; sous la poignée d'essai `EVE_MICRO`, le micro d'essai ([`micro_d_essai`]).
    pub fn ouvrir() -> Result<Micro, String> {
        if let Some(dossier) = std::env::var_os("EVE_MICRO").filter(|d| !d.is_empty()) {
            return Ok(micro_d_essai(PathBuf::from(dossier)));
        }
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
        Ok(Micro { _flux: Some(flux), partage, taux, nom })
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

/// Le micro d'essai (`EVE_MICRO=<dossier>`) : à la place du micro du poste, Eve entend les sons WAV qu'on dépose dans
/// ce dossier, un à un dans l'ordre de leur nom, en temps réel, comme dits devant elle — chacun renommé `.entendu`
/// quand il commence —, sur un léger souffle de micro (vers -70 dBFS). Le tour de parole s'essaie ainsi de bout en
/// bout, la touche (les gestes de la fenêtre) comme l'écoute continue, sans rien jouer dans la pièce. Jamais posée par
/// Eve elle-même.
fn micro_d_essai(dossier: PathBuf) -> Micro {
    let partage = Arc::new(Mutex::new(Capture::nouvelle(CADENCE)));
    let p = partage.clone();
    let nom = format!("micro d'essai ({})", dossier.display());
    thread::spawn(move || {
        let pas = CADENCE as usize / 100;
        let (mut son, mut prochain, mut regarde) = (VecDeque::new(), Instant::now(), Instant::now());
        let mut hasard = 0x2545_f491_4f6c_dd1d_u64;
        // Jusqu'à ce que le micro soit lâché (une réouverture en fait un autre).
        while Arc::strong_count(&p) > 1 {
            if son.is_empty() && regarde.elapsed() >= Duration::from_millis(100) {
                regarde = Instant::now();
                if let Some(f) = premier_wav(&dossier) {
                    match lire_wav(&f) {
                        Ok(s) => son.extend(s),
                        Err(e) => crate::journal::ecrire(&format!("micro d'essai : {} ({e})", f.display())),
                    }
                    let _ = std::fs::rename(&f, f.with_extension("entendu"));
                }
            }
            let bloc: Vec<f32> = (0..pas)
                .map(|_| {
                    hasard ^= hasard << 13;
                    hasard ^= hasard >> 7;
                    hasard ^= hasard << 17;
                    let souffle = ((hasard >> 40) as f32 / (1u64 << 24) as f32 - 0.5) * 0.001;
                    son.pop_front().unwrap_or(0.0) + souffle
                })
                .collect();
            recevoir(&p, &bloc, 1, de_f32);
            prochain += Duration::from_millis(10);
            thread::sleep(prochain.saturating_duration_since(Instant::now()));
        }
    });
    Micro { _flux: None, partage, taux: CADENCE, nom }
}

/// Le premier son WAV d'un dossier, dans l'ordre des noms.
fn premier_wav(dossier: &Path) -> Option<PathBuf> {
    let mut wavs: Vec<PathBuf> = std::fs::read_dir(dossier)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("wav")))
        .collect();
    wavs.sort();
    wavs.into_iter().next()
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

/// Une phrase de l'écoute continue, rejouée sur un son ([`rejouer_l_ecoute`]) : quand le détecteur l'a entendue
/// commencer et quand il l'a finie (en secondes du son), et son son, tel qu'il part à l'oreille.
pub struct PhraseEntendue {
    pub debut: f32,
    pub fin: f32,
    pub son: Vec<f32>,
}

/// L'écoute continue rejouée sur un son (mono, à `taux`), comme le micro la mène ([`recevoir`] : la demi-seconde
/// d'avance, le détecteur et son silence de fin, `fin` secondes) : les phrases qu'il en tire. Pour l'essai du tour de
/// parole (`eve --essai-tour`).
pub fn rejouer_l_ecoute(son: &[f32], taux: u32, fin: f32) -> Vec<PhraseEntendue> {
    let mut d = Detecteur { fin, ..Detecteur::default() };
    let (mut avance, mut sorties, mut rendues) = (VecDeque::new(), VecDeque::new(), Vec::new());
    let mut debut = 0.0;
    for (i, &m) in son.iter().enumerate() {
        avance.push_back(m);
        if avance.len() > taux as usize / 2 {
            avance.pop_front();
        }
        let avant = d.en_parole;
        d.ajouter(m, taux, &avance, &mut sorties);
        let t = (i + 1) as f32 / taux as f32;
        if d.en_parole && !avant {
            debut = t;
        }
        if let Some(p) = sorties.pop_front() {
            rendues.push(PhraseEntendue { debut, fin: t, son: p });
        }
    }
    rendues
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
    pcm_du_wav(&std::fs::read(chemin).map_err(|e| format!("{} : {e}", chemin.display()))?)
}

/// Un WAV en mémoire (PCM 16 bits, toutes cadences, mono ou stéréo), à 16 kHz mono : celui d'un fichier d'essai, ou la
/// voix que le téléphone envoie ([`crate::telephone`]).
pub fn pcm_du_wav(o: &[u8]) -> Result<Vec<f32>, String> {
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

    /// Le micro d'essai joue en temps réel le son qu'on dépose dans son dossier : tenu par la touche, il le rend ; le
    /// son joué se renomme ; lâché, son fil s'arrête.
    #[test]
    fn le_micro_d_essai_joue_ce_qu_on_depose() {
        let dossier = std::env::temp_dir().join(format!("eve-micro-essai-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dossier);
        std::fs::create_dir_all(&dossier).unwrap();
        // 0,6 s d'un son fort, à 16 kHz.
        let echantillons: Vec<i16> = (0..9_600).map(|i| (8_000.0 * (i as f32 * 0.05).sin()) as i16).collect();
        let mut wav = Vec::new();
        for (bloc, valeur) in [(b"RIFF", 36 + 19_200u32), (b"WAVE", 0)] {
            wav.extend_from_slice(bloc);
            if valeur > 0 {
                wav.extend_from_slice(&valeur.to_le_bytes());
            }
        }
        wav.extend_from_slice(b"fmt ");
        for v in [16u32, 1 | (1 << 16), 16_000, 32_000, 2 | (16 << 16)] {
            wav.extend_from_slice(&v.to_le_bytes());
        }
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&19_200u32.to_le_bytes());
        wav.extend(echantillons.iter().flat_map(|e| e.to_le_bytes()));
        let m = micro_d_essai(dossier.clone());
        m.debut();
        std::fs::write(dossier.join("a.tmp"), &wav).unwrap();
        std::fs::rename(dossier.join("a.tmp"), dossier.join("a.wav")).unwrap();
        // Le son se joue en temps réel (0,6 s) dès que le fil du micro d'essai le prend — il le renomme alors : on attend
        // qu'il le prenne (cinq secondes au plus), puis la durée du son et une marge. Sous la charge, le fil le prend
        // plus tard : le 28 octobre, une attente fixe de 1,2 s depuis le dépôt n'a pas suffi deux fois.
        let debut = std::time::Instant::now();
        while !dossier.join("a.entendu").is_file() && debut.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(50));
        }
        std::thread::sleep(Duration::from_millis(1_000));
        let entendu = m.fin();
        assert!(dossier.join("a.entendu").is_file(), "le son joué se renomme");
        assert!(entendu.len() > 15_000 && rms(&entendu) > 0.05, "{} échantillons, {}", entendu.len(), rms(&entendu));
        assert!(!m.en_panne());
        drop(m);
        let _ = std::fs::remove_dir_all(&dossier);
    }

    /// L'écoute rejouée sur un son : une pause plus courte que le silence de fin ne coupe pas la phrase ; plus longue,
    /// elle la coupe — chaque morceau avec ses instants, son avance comprise.
    #[test]
    fn l_ecoute_rejouee_coupe_aux_longues_pauses() {
        let taux = 16_000;
        // 0,5 s de silence, 1 s de « parole », une pause, 1 s de « parole », 2 s de silence.
        let son = |pause: f32| -> Vec<f32> {
            let n = |s: f32| (s * taux as f32) as usize;
            let parole = |k: usize| (0..k).map(|i| 0.3 * (i as f32 * 0.05).sin()).collect::<Vec<f32>>();
            [vec![0.001; n(0.5)], parole(n(1.0)), vec![0.001; n(pause)], parole(n(1.0)), vec![0.001; n(2.0)]].concat()
        };
        let une = rejouer_l_ecoute(&son(0.5), taux, SILENCE_DE_FIN);
        assert_eq!(une.len(), 1, "une pause de 0,5 s ne coupe pas");
        assert!(
            (une[0].debut - 0.56).abs() < 0.03 && (une[0].fin - 3.7).abs() < 0.05,
            "{} {}",
            une[0].debut,
            une[0].fin
        );
        let deux = rejouer_l_ecoute(&son(1.0), taux, SILENCE_DE_FIN);
        assert_eq!(deux.len(), 2, "une pause de 1 s coupe");
        assert!(
            (deux[0].fin - 2.2).abs() < 0.05 && (deux[1].debut - 2.56).abs() < 0.03,
            "{} {}",
            deux[0].fin,
            deux[1].debut
        );
        assert_eq!(rejouer_l_ecoute(&son(1.0), taux, 1.2).len(), 1, "un silence de fin plus long ne la coupe pas");
    }
}
