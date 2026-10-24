//! L'oreille : whisper.cpp, sur le poste — rien de ce qui se dit ne part en ligne pour être transcrit.
//!
//! Deux moteurs, choisis au démarrage :
//! - **la carte graphique** : le serveur officiel de whisper.cpp, compilé pour CUDA (`whisper-gpu\whisper-server.exe`
//!   du dossier local), lancé en processus enfant — sans fenêtre, en priorité haute, rattaché au groupe d'Eve, à
//!   l'écoute de 127.0.0.1 seulement. Le fidèle (small) y transcrit une phrase de 10,8 s en 1,3 s, contre 8,6 s sur le
//!   processeur (une GTX 1050 Ti et un processeur de 2012, sans AVX2), et y cherche aussi « Eve » (0,25 s
//!   pour le début d'une phrase).
//! - **le processeur** : whisper.cpp compilé dans le programme (whisper-rs), avec deux modèles — le fidèle pour les
//!   demandes, le rapide (base) pour chercher « Eve ». C'est le repli : sans le serveur, avec `EVE_SANS_CARTE`, ou
//!   quand la carte ne répond plus. Ses modèles ne se chargent qu'alors : la mémoire du poste ne se prend pas deux fois.
//!
//! Le serveur qui tombe en pleine séance est relancé, une fois ; s'il ne repart pas (ou retombe dans les dix minutes),
//! le processeur prend la suite de la séance, et la phrase en cours se transcrit par lui : aucune n'est perdue.
//!
//! Le mot d'éveil : les 2,5 premières secondes de chaque phrase de l'écoute continue disent si elle appelle Eve. Une
//! phrase qui ne commence pas par « Eve » s'oublie aussitôt, sans être écrite nulle part ; une qui commence par lui se
//! transcrit en entier par le fidèle — sauf quand la première écoute l'a déjà entendue tout entière.
//!
//! Chaque phrase s'encode sur sa durée réelle, pas sur les 30 s fixes de Whisper ([`fenetre_d_encodage`]).

use std::borrow::Cow;
use std::collections::VecDeque;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStderr, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use eve::parole;
use serde_json::Value;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperState};

use crate::chef::Evt;
use crate::journal;
use crate::micro::CADENCE;
#[cfg(test)]
use crate::reglages::dossier_local;

/// Ce que le chef demande à l'oreille. `depuis` : la fin de la parole (la touche relâchée, la phrase finie), rendue
/// avec le texte — le temps de l'oreille se compte de là, file d'attente comprise (`eve --mesures`).
pub enum Demande {
    /// Une demande à transcrire (la touche, ou la réponse à « Oui ? », ou « je confirme ») ; `continue_` : entendue par
    /// l'écoute continue (sans touche), rendue telle quelle dans `Evt::Transcrit`.
    Commande { audio: Vec<f32>, continue_: bool, depuis: Instant },
    /// Une phrase de l'écoute continue : commence-t-elle par « Eve » ?
    Eveil { audio: Vec<f32>, depuis: Instant },
}

/// Ce qu'il faut entendre au début d'une phrase pour y chercher « Eve » : les premières secondes suffisent.
const DEBUT_EVEIL: usize = CADENCE as usize * 5 / 2;

// Les ports essayés pour le serveur de la carte graphique (le premier libre), sur 127.0.0.1 seulement.
const PORT_PREMIER: u16 = 8178;
const PORT_DERNIER: u16 = 8190;
/// L'attente du serveur : 2 à 3 s d'ordinaire ; mais après une mise à jour du pilote NVIDIA, son premier lancement
/// compile le code CUDA (environ 40 s).
const ATTENTE_SERVEUR: Duration = Duration::from_secs(90);
/// Une transcription sur la carte : 1,3 s pour 10,8 s de parole ; au-delà de 30 s, le serveur est tenu pour bloqué.
const DELAI_TRANSCRIPTION: Duration = Duration::from_secs(30);
/// La plus petite fenêtre des grands modèles sur la carte. Mesuré le 2026-10-23 avec large-v3-turbo, sur 16 phrases de
/// 1,8 à 3,6 s (les deux voix de Piper, un souffle de webcam) : à 256 trames (la fenêtre de ces phrases), 1 juste sur
/// 16, 7,1 s en médiane, des boucles jusqu'à 11 s ; à 512, 12 justes, mais 6 au-delà de 2 s (jusqu'à 7,8 s) ; à 768,
/// 12 justes, 0,78 s en médiane, 0,86 s au pire ; pleine (1500), 12 justes, 1,63 s. Chez l'Assistante famille, le même
/// jour : 3 phrases de moins de 3 s sur 4 bouclaient ou s'inventaient.
const PLANCHER_DES_GRANDS: i32 = 768;
/// L'ouverture d'une connexion locale : immédiate quand le serveur écoute ; mais Windows met 2 s à refuser un port
/// fermé (mesuré le 2026-10-22) — d'où la sonde à délai court du serveur qui démarre.
const DELAI_CONNEXION: Duration = Duration::from_secs(2);
const DELAI_SONDE: Duration = Duration::from_millis(250);
/// Une réponse de `/health`.
const DELAI_SANTE: Duration = Duration::from_secs(2);
/// Un serveur qui retombe moins de dix minutes après sa relance ne repart pas : le processeur prend la suite.
const RECHUTE: Duration = Duration::from_secs(600);
/// Sans fenêtre (CREATE_NO_WINDOW) et en priorité haute (HIGH_PRIORITY_CLASS) : en priorité normale, la charge du
/// poste ralentit le serveur (1,9 s au lieu de 1,3).
#[cfg(windows)]
const DRAPEAUX_SERVEUR: u32 = 0x0800_0080;
/// La frontière des formulaires multipart ; rien dans un WAV ni dans l'amorce ne la reproduit par hasard.
const FRONTIERE: &str = "----EveFrontiere7d3c9b1f0a6e4258";
// Les bornes d'une réponse HTTP (une ligne d'en-tête, le corps), et les lignes gardées de la sortie du serveur.
const LIGNE_MAX: u64 = 8 * 1024;
const CORPS_MAX: usize = 16 << 20;
const TRACES_MAX: usize = 30;

/// Démarre l'oreille ; les résultats reviennent en `Evt::Transcrit`, `Evt::EveilEntendu` et `Evt::Eveil`. `fidele` :
/// le modèle des demandes sur le processeur ; `carte` : celui de la carte graphique (souvent un plus grand, qui y va
/// aussi vite) ; `rapide` : celui qui cherche « Eve » sur le processeur.
pub fn demarrer(
    fidele: PathBuf,
    carte: PathBuf,
    rapide: PathBuf,
    vocabulaire: String,
    rapport: Sender<Evt>,
) -> Sender<Demande> {
    let (tx, rx) = mpsc::channel::<Demande>();
    thread::spawn(move || {
        whisper_rs::install_logging_hooks();
        let mut oreille = Oreille::ouvrir(fidele, carte, rapide, rapport);
        if matches!(oreille.moteur, Moteur::Aucun) {
            return;
        }
        let amorce_eveil = format!("Eve, {vocabulaire}");
        for demande in rx {
            match demande {
                Demande::Commande { audio, continue_, depuis } => {
                    let t0 = Instant::now();
                    let texte = oreille.entendre(&audio, &vocabulaire, Modele::Fidele);
                    let _ = oreille.rapport.send(Evt::Transcrit {
                        texte,
                        secondes_audio: audio.len() as f32 / CADENCE as f32,
                        secondes_calcul: t0.elapsed().as_secs_f32(),
                        continue_,
                        depuis,
                    });
                }
                Demande::Eveil { audio, depuis } => oreille.eveil(&audio, &amorce_eveil, depuis),
            }
        }
    });
    tx
}

/// Le modèle voulu pour une phrase : sur la carte graphique, c'est toujours le fidèle.
#[derive(Clone, Copy)]
enum Modele {
    Fidele,
    /// Pour chercher « Eve » : le rapide, sur le processeur.
    Rapide,
}

enum Moteur {
    Carte(Carte),
    Processeur(Processeur),
    /// Rien ne se charge : les demandes restent sans texte.
    Aucun,
}

/// L'oreille, dans son fil : le moteur du moment, et de quoi passer de l'un à l'autre.
struct Oreille {
    moteur: Moteur,
    fidele: PathBuf,
    /// Le modèle du serveur de la carte graphique.
    carte: PathBuf,
    rapide: PathBuf,
    rapport: Sender<Evt>,
    /// La dernière relance du serveur de la carte graphique.
    relance: Option<Instant>,
}

impl Oreille {
    /// La carte graphique, si son serveur est là et que `EVE_SANS_CARTE` n'est pas posée ; sinon — ou s'il ne démarre
    /// pas — le processeur. Le moteur prêt s'annonce en `Evt::Oreille`.
    fn ouvrir(fidele: PathBuf, carte: PathBuf, rapide: PathBuf, rapport: Sender<Evt>) -> Oreille {
        let mut oreille = Oreille { moteur: Moteur::Aucun, fidele, carte, rapide, rapport, relance: None };
        let serveur = crate::reglages::ressource("whisper-gpu").join("whisper-server.exe");
        if !serveur.is_file() || std::env::var_os("EVE_SANS_CARTE").is_some() {
            oreille.passer_au_processeur("");
            return oreille;
        }
        let nom = nom_du_modele(&oreille.carte);
        match Carte::lancer(&serveur, &oreille.carte) {
            Ok(carte) => {
                journal::ecrire(&format!("oreille : Whisper {nom} sur la carte graphique ({})", carte.resume()));
                let pret = en_francais(carte.demarrage + carte.echauffement);
                let _ = oreille
                    .rapport
                    .send(Evt::Oreille(Ok(format!("Whisper {nom} sur la carte graphique, prêt en {pret} s"))));
                oreille.moteur = Moteur::Carte(carte);
            }
            Err(e) => {
                journal::ecrire(&format!("oreille : la carte graphique ne démarre pas ({e})"));
                oreille.passer_au_processeur("la carte graphique ne démarre pas");
            }
        }
        oreille
    }

    /// Passe au processeur : le serveur de la carte s'arrête d'abord (sa mémoire se libère), puis le fidèle et le
    /// rapide se chargent. `raison` : pourquoi pas la carte graphique (vide quand elle n'est pas là).
    fn passer_au_processeur(&mut self, raison: &str) {
        self.moteur = Moteur::Aucun;
        let nom = nom_du_modele(&self.fidele);
        match Processeur::ouvrir(&self.fidele, &self.rapide) {
            Ok((p, duree)) => {
                let rapide = if p.rapide.is_some() { "avec le rapide" } else { "sans le rapide" };
                journal::ecrire(&format!(
                    "oreille : Whisper {nom} sur le processeur, chargé en {duree:.1} s ({rapide})"
                ));
                let detail = if raison.is_empty() {
                    format!("Whisper {nom} sur le processeur, chargé en {} s", en_francais(duree))
                } else {
                    format!("Whisper {nom} sur le processeur ({raison})")
                };
                let _ = self.rapport.send(Evt::Oreille(Ok(detail)));
                self.moteur = Moteur::Processeur(p);
            }
            Err(e) => {
                journal::ecrire(&format!("oreille : {e}"));
                let _ = self.rapport.send(Evt::Oreille(Err(e)));
            }
        }
    }

    /// Une phrase (16 kHz, mono) en texte, par le moteur du moment. La carte qui ne répond plus est relancée ; si elle
    /// ne repart pas, la phrase se transcrit sur le processeur, qui garde la suite de la séance.
    fn entendre(&mut self, audio: &[f32], amorce: &str, modele: Modele) -> String {
        if let Moteur::Carte(carte) = &mut self.moteur {
            match carte.transcrire(audio, amorce) {
                Ok(texte) => return texte,
                Err(e) => {
                    journal::ecrire(&format!("oreille : la carte graphique ne répond plus ({e})"));
                    if let Some(texte) = self.relancer(audio, amorce) {
                        return texte;
                    }
                    journal::ecrire(
                        "oreille : la carte graphique ne repart pas ; le processeur prend la suite de la séance",
                    );
                    self.passer_au_processeur("la carte graphique ne répond plus");
                }
            }
        }
        match &mut self.moteur {
            Moteur::Processeur(p) => {
                let fils = p.fils;
                let etat = match modele {
                    Modele::Rapide => p.rapide.as_mut().unwrap_or(&mut p.fidele),
                    Modele::Fidele => &mut p.fidele,
                };
                transcrire(etat, audio, amorce, fils)
            }
            Moteur::Carte(_) | Moteur::Aucun => String::new(),
        }
    }

    /// Relance le serveur de la carte graphique et lui redonne la phrase — une fois : pas s'il retombe dans les dix
    /// minutes qui suivent sa dernière relance.
    fn relancer(&mut self, audio: &[f32], amorce: &str) -> Option<String> {
        if self.relance.is_some_and(|r| r.elapsed() < RECHUTE) {
            return None;
        }
        self.relance = Some(Instant::now());
        let Moteur::Carte(carte) = &mut self.moteur else { return None };
        carte.arreter();
        match Carte::lancer(&carte.serveur, &carte.modele) {
            Ok(nouvelle) => {
                journal::ecrire(&format!("oreille : la carte graphique est relancée ({})", nouvelle.resume()));
                *carte = nouvelle;
            }
            Err(e) => {
                journal::ecrire(&format!("oreille : la relance échoue ({e})"));
                return None;
            }
        }
        match carte.transcrire(audio, amorce) {
            Ok(texte) => Some(texte),
            Err(e) => {
                journal::ecrire(&format!("oreille : la carte graphique relancée ne répond pas ({e})"));
                None
            }
        }
    }

    /// Une phrase de l'écoute continue : si elle commence par « Eve », `Evt::EveilEntendu`, puis la demande qui suit en
    /// `Evt::Eveil` ; sinon rien, et elle s'oublie.
    fn eveil(&mut self, audio: &[f32], amorce: &str, depuis: Instant) {
        let entiere = audio.len() <= DEBUT_EVEIL;
        let entete = self.entendre(&audio[..audio.len().min(DEBUT_EVEIL)], amorce, Modele::Rapide);
        let Some(reste) = parole::appel_a_eve(&parole::nettoyer_transcription(&entete)) else { return };
        let _ = self.rapport.send(Evt::EveilEntendu);
        let demande = if premiere_ecoute_suffit(entiere, self.eveil_par_le_fidele(), &reste) {
            reste
        } else {
            let complet = parole::nettoyer_transcription(&self.entendre(audio, amorce, Modele::Fidele));
            parole::appel_a_eve(&complet).unwrap_or(complet)
        };
        let _ = self.rapport.send(Evt::Eveil { demande, depuis });
    }

    /// Le fidèle cherche-t-il aussi « Eve » ? Sur la carte graphique, oui ; sur le processeur, quand le rapide manque.
    fn eveil_par_le_fidele(&self) -> bool {
        match &self.moteur {
            Moteur::Carte(_) => true,
            Moteur::Processeur(p) => p.rapide.is_none(),
            Moteur::Aucun => false,
        }
    }
}

/// La première écoute (le début de la phrase) suffit-elle ? Seulement si elle a entendu la phrase entière : faite par
/// le fidèle, elle ne se refait pas ; « Eve » seul (rien après) ou un arrêt (« Eve, stop ») part tout de suite, sans
/// attendre le fidèle.
fn premiere_ecoute_suffit(entiere: bool, par_le_fidele: bool, reste: &str) -> bool {
    entiere && (par_le_fidele || reste.is_empty() || parole::est_un_arret(reste))
}

/// Whisper dans le programme, sur le processeur.
struct Processeur {
    fidele: WhisperState,
    rapide: Option<WhisperState>,
    fils: i32,
}

impl Processeur {
    /// Charge le fidèle, puis le rapide — facultatif : sans lui, le fidèle cherche aussi « Eve » (plus lentement) ; rend
    /// aussi la durée du chargement du fidèle.
    fn ouvrir(fidele: &Path, rapide: &Path) -> Result<(Processeur, f32), String> {
        let (fidele, duree) = charger(fidele)?;
        let rapide = charger(rapide).ok().map(|(etat, _)| etat);
        Ok((Processeur { fidele, rapide, fils: fils_de_calcul() }, duree))
    }
}

/// Le serveur de whisper.cpp sur la carte graphique, en processus enfant : rattaché au groupe d'Eve, il se ferme avec
/// elle ; il s'arrête aussi quand la carte est lâchée. L'essai de l'oreille s'en sert aussi (`eve --essai-oreille`).
pub(crate) struct Carte {
    serveur: PathBuf,
    modele: PathBuf,
    port: u16,
    enfant: Child,
    traces: Arc<Mutex<Traces>>,
    /// Les secondes jusqu'à ce que `/health` réponde, puis celles de l'échauffement.
    demarrage: f32,
    echauffement: f32,
    /// La plus petite fenêtre d'encodage que son modèle tient ([`plancher_de_fenetre`]).
    plancher: i32,
}

/// Ce qu'on garde de la sortie d'erreur du serveur — il n'y écrit jamais ce qu'il transcrit.
#[derive(Default)]
struct Traces {
    /// Les dernières lignes (CUDA, le chargement) : la dernière accompagne un échec au journal.
    lignes: VecDeque<String>,
    /// Le modèle est-il sur la carte (« CUDA0 ») ? Sans pilote NVIDIA, ggml se rabat en silence sur le processeur, et
    /// le serveur, réglé sur un seul fil, y serait bien plus lent que le repli.
    sur_la_carte: bool,
}

impl Carte {
    /// Lance le serveur sur le premier port libre, attend qu'il réponde (90 s au plus), puis l'échauffe d'une seconde de
    /// silence.
    pub(crate) fn lancer(serveur: &Path, modele: &Path) -> Result<Carte, String> {
        if !modele.is_file() {
            return Err(format!("le modèle de Whisper est introuvable : {}", modele.display()));
        }
        let port = (PORT_PREMIER..=PORT_DERNIER)
            .find(|&p| TcpListener::bind((Ipv4Addr::LOCALHOST, p)).is_ok())
            .ok_or_else(|| format!("aucun port libre de {PORT_PREMIER} à {PORT_DERNIER}"))?;
        let t0 = Instant::now();
        let mut c = Command::new(serveur);
        if let Some(dossier) = serveur.parent() {
            c.current_dir(dossier);
        }
        // Sans « -l fr », il transcrit en anglais ; un seul fil est son meilleur réglage (le calcul est sur la carte).
        c.arg("-m")
            .arg(modele)
            .args(["-l", "fr", "-t", "1", "-bs", "1", "-bo", "1", "-nt", "-sns", "--host", "127.0.0.1", "--port"])
            .arg(port.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            c.creation_flags(DRAPEAUX_SERVEUR);
        }
        let mut enfant = c.spawn().map_err(|e| format!("le serveur ne se lance pas : {e}"))?;
        crate::groupe::rattacher(&enfant);
        let traces = Arc::new(Mutex::new(Traces::default()));
        if let Some(erreurs) = enfant.stderr.take() {
            let traces = Arc::clone(&traces);
            thread::spawn(move || garder_les_traces(erreurs, &traces));
        }
        let mut carte = Carte {
            serveur: serveur.to_path_buf(),
            modele: modele.to_path_buf(),
            port,
            enfant,
            traces,
            demarrage: 0.0,
            echauffement: 0.0,
            plancher: plancher_de_fenetre(modele),
        };
        carte.attendre(ATTENTE_SERVEUR)?;
        carte.demarrage = t0.elapsed().as_secs_f32();
        // Le modèle chargé, ses lignes sont dites ; elles arrivent par le fil des traces : un instant de grâce.
        let fin = Instant::now() + Duration::from_secs(2);
        while !carte.traces.lock().is_ok_and(|t| t.sur_la_carte) {
            if Instant::now() >= fin {
                return Err(
                    "le serveur ne trouve pas la carte graphique (CUDA) : il tournerait sur le processeur".into()
                );
            }
            thread::sleep(Duration::from_millis(50));
        }
        let t1 = Instant::now();
        carte.transcrire(&[0.0; CADENCE as usize], "").map_err(|e| format!("l'échauffement échoue : {e}"))?;
        carte.echauffement = t1.elapsed().as_secs_f32();
        Ok(carte)
    }

    /// Attend que `/health` réponde « ok » ; échoue si le serveur s'arrête, ou tarde au-delà de `delai`.
    fn attendre(&mut self, delai: Duration) -> Result<(), String> {
        let fin = Instant::now() + delai;
        loop {
            if let Some(e) = self.est_arrete() {
                return Err(e);
            }
            if let Ok((200, corps)) = requete(self.port, "GET", "/health", None, &[], DELAI_SANTE, DELAI_SONDE)
                && String::from_utf8_lossy(&corps).contains("\"ok\"")
            {
                return Ok(());
            }
            if Instant::now() >= fin {
                return Err(format!("le serveur ne répond pas après {} s{}", delai.as_secs(), self.trace()));
            }
            thread::sleep(Duration::from_millis(100));
        }
    }

    /// Le serveur s'est-il arrêté ? `Some` : ce qu'on en sait, avec sa dernière trace.
    fn est_arrete(&mut self) -> Option<String> {
        match self.enfant.try_wait() {
            Ok(None) => None,
            Ok(Some(statut)) => Some(format!("le serveur s'est arrêté ({}){}", code_de_sortie(statut), self.trace())),
            Err(e) => Some(format!("le serveur ne se laisse plus suivre ({e})")),
        }
    }

    /// Une phrase en texte ; une erreur si le serveur s'est arrêté, ne répond pas, ou répond mal.
    pub(crate) fn transcrire(&mut self, audio: &[f32], amorce: &str) -> Result<String, String> {
        match self.est_arrete() {
            Some(e) => Err(e),
            None => inference(self.port, audio, amorce, self.plancher),
        }
    }

    /// La dernière ligne de la sortie d'erreur du serveur (« — … ») ; vide s'il n'a rien dit.
    fn trace(&self) -> String {
        let derniere = self.traces.lock().ok().and_then(|t| t.lignes.back().cloned());
        derniere.map(|l| format!(" — {l}")).unwrap_or_default()
    }

    pub(crate) fn resume(&self) -> String {
        format!("port {}, prêt en {:.1} s, échauffé en {:.1} s", self.port, self.demarrage, self.echauffement)
    }

    /// Arrête le serveur, s'il tourne encore, et attend sa fin.
    fn arreter(&mut self) {
        let _ = self.enfant.kill();
        let _ = self.enfant.wait();
    }
}

impl Drop for Carte {
    fn drop(&mut self) {
        self.arreter();
    }
}

/// Lit la sortie d'erreur du serveur jusqu'à sa fin — un tuyau qu'on ne lit pas finit par le bloquer — et en garde les
/// dernières lignes.
fn garder_les_traces(erreurs: ChildStderr, traces: &Mutex<Traces>) {
    let mut lecteur = BufReader::new(erreurs);
    let mut ligne = Vec::new();
    loop {
        ligne.clear();
        match lecteur.by_ref().take(LIGNE_MAX).read_until(b'\n', &mut ligne) {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                let texte = String::from_utf8_lossy(&ligne).trim().to_string();
                if !texte.is_empty()
                    && let Ok(mut t) = traces.lock()
                {
                    t.sur_la_carte |= texte.contains("CUDA0");
                    t.lignes.push_back(texte);
                    while t.lignes.len() > TRACES_MAX {
                        t.lignes.pop_front();
                    }
                }
            }
        }
    }
}

/// Le code de sortie d'un processus ; en hexadécimal au-delà de 255 (0xc0000005 : un plantage).
fn code_de_sortie(statut: ExitStatus) -> String {
    match statut.code() {
        Some(c) if (0..=255).contains(&c) => format!("code {c}"),
        Some(c) => format!("code {:#010x}", c as u32),
        None => "sans code".into(),
    }
}

/// Une phrase transcrite par le serveur : le son en WAV, un formulaire multipart, la réponse en JSON. `plancher` : la
/// plus petite fenêtre que le modèle tient.
fn inference(port: u16, audio: &[f32], amorce: &str, plancher: i32) -> Result<String, String> {
    let audio_ctx = audio_ctx_carte(fenetre_voulue(audio.len(), plancher)).to_string();
    let mut champs = vec![
        ("language", "fr"),
        ("audio_ctx", audio_ctx.as_str()),
        ("best_of", "1"),
        ("beam_size", "1"),
        ("no_timestamps", "true"),
        ("suppress_nst", "true"),
        ("temperature", "0.0"),
        ("temperature_inc", "0.2"),
        ("response_format", "json"),
    ];
    if !amorce.is_empty() {
        champs.push(("prompt", amorce));
    }
    let corps = corps_multipart(FRONTIERE, &wav_16k(&au_moins_une_seconde(audio)), &champs);
    let sorte = format!("multipart/form-data; boundary={FRONTIERE}");
    let (code, reponse) =
        requete(port, "POST", "/inference", Some(sorte.as_str()), &corps, DELAI_TRANSCRIPTION, DELAI_CONNEXION)
            .map_err(|e| format!("le serveur ne répond pas ({e})"))?;
    if code != 200 {
        let debut = String::from_utf8_lossy(&reponse[..reponse.len().min(200)]).into_owned();
        return Err(format!("le serveur répond {code} : {debut}"));
    }
    texte_de_la_reponse(&reponse)
}

/// La fenêtre d'encodage sur la carte. Avec la « flash attention » du serveur, une fenêtre qui n'est pas un multiple de
/// 256 rend du texte faux (la phrase de 10,8 s devenait « Voie N. » trois fois sur quatre) ; arrondie au multiple de
/// 256 supérieur, elle est juste (éprouvé sur 40 requêtes, le 2026-10-22). Au-delà de 1280 : la fenêtre pleine (0).
fn audio_ctx_carte(fenetre: i32) -> i32 {
    if fenetre <= 0 || fenetre > 1280 { 0 } else { (fenetre + 255) / 256 * 256 }
}

/// Le texte de la réponse du serveur (`{"text":" …"}`) : un segment par ligne, rejoints d'une espace comme sur le
/// processeur.
fn texte_de_la_reponse(corps: &[u8]) -> Result<String, String> {
    let v: Value = serde_json::from_slice(corps).map_err(|e| format!("réponse illisible ({e})"))?;
    if let Some(e) = v.get("error") {
        return Err(format!("le serveur refuse : {}", e.as_str().map(str::to_string).unwrap_or_else(|| e.to_string())));
    }
    let texte = v.get("text").and_then(Value::as_str).ok_or("réponse sans texte")?;
    Ok(texte.lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" "))
}

/// Un son (16 kHz, mono) en fichier WAV : PCM 16 bits, l'en-tête de 44 octets.
fn wav_16k(audio: &[f32]) -> Vec<u8> {
    let donnees = (audio.len() * 2) as u32;
    let mut w = Vec::with_capacity(44 + audio.len() * 2);
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36 + donnees).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes()); // PCM
    w.extend_from_slice(&1u16.to_le_bytes()); // mono
    w.extend_from_slice(&CADENCE.to_le_bytes());
    w.extend_from_slice(&(CADENCE * 2).to_le_bytes()); // octets par seconde
    w.extend_from_slice(&2u16.to_le_bytes()); // octets par échantillon
    w.extend_from_slice(&16u16.to_le_bytes()); // bits
    w.extend_from_slice(b"data");
    w.extend_from_slice(&donnees.to_le_bytes());
    for &x in audio {
        // La conversion sature : 1,0 donne 32 767.
        w.extend_from_slice(&((x * 32_768.0).round() as i16).to_le_bytes());
    }
    w
}

/// Le corps d'un formulaire multipart/form-data : le son (le champ `file`), puis les champs de texte.
fn corps_multipart(frontiere: &str, wav: &[u8], champs: &[(&str, &str)]) -> Vec<u8> {
    let mut c = Vec::with_capacity(wav.len() + 2048);
    c.extend_from_slice(
        format!(
            "--{frontiere}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"phrase.wav\"\r\n\
             Content-Type: audio/wav\r\n\r\n"
        )
        .as_bytes(),
    );
    c.extend_from_slice(wav);
    c.extend_from_slice(b"\r\n");
    for (nom, valeur) in champs {
        c.extend_from_slice(
            format!("--{frontiere}\r\nContent-Disposition: form-data; name=\"{nom}\"\r\n\r\n{valeur}\r\n").as_bytes(),
        );
    }
    c.extend_from_slice(format!("--{frontiere}--\r\n").as_bytes());
    c
}

/// La tête d'une requête HTTP/1.1 : la connexion se ferme après la réponse.
fn tete_de_requete(methode: &str, chemin: &str, port: u16, sorte: Option<&str>, longueur: usize) -> String {
    let mut t = format!("{methode} {chemin} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n");
    if let Some(s) = sorte {
        t.push_str(&format!("Content-Type: {s}\r\n"));
    }
    if methode != "GET" || longueur > 0 {
        t.push_str(&format!("Content-Length: {longueur}\r\n"));
    }
    t.push_str("Connection: close\r\n\r\n");
    t
}

/// Une requête au serveur local ; rend le code et le corps de la réponse. `delai` vaut pour la requête entière,
/// `connexion` pour l'ouverture seule.
fn requete(
    port: u16,
    methode: &str,
    chemin: &str,
    sorte: Option<&str>,
    corps: &[u8],
    delai: Duration,
    connexion: Duration,
) -> io::Result<(u16, Vec<u8>)> {
    let fin = Instant::now() + delai;
    let mut flux = TcpStream::connect_timeout(&SocketAddr::from((Ipv4Addr::LOCALHOST, port)), connexion.min(delai))?;
    flux.set_nodelay(true)?;
    flux.set_write_timeout(Some(delai))?;
    flux.write_all(tete_de_requete(methode, chemin, port, sorte, corps.len()).as_bytes())?;
    flux.write_all(corps)?;
    lire_reponse(&mut BufReader::new(AvecEcheance { flux, fin }))
}

/// Un flux dont les lectures partagent une même échéance : le délai vaut pour la réponse entière, pas pour chaque
/// lecture.
struct AvecEcheance {
    flux: TcpStream,
    fin: Instant,
}

impl Read for AvecEcheance {
    fn read(&mut self, tampon: &mut [u8]) -> io::Result<usize> {
        let reste = self.fin.saturating_duration_since(Instant::now());
        if reste.is_zero() {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "pas de réponse dans le délai"));
        }
        self.flux.set_read_timeout(Some(reste))?;
        self.flux.read(tampon)
    }
}

/// Une réponse HTTP/1.1 : la ligne d'état, les en-têtes, puis le corps — par sa longueur (`Content-Length`), en
/// morceaux (`Transfer-Encoding: chunked`), ou jusqu'à la fermeture. Une réponse tronquée est une erreur.
fn lire_reponse<R: BufRead>(r: &mut R) -> io::Result<(u16, Vec<u8>)> {
    let etat = lire_ligne(r)?;
    let code = etat
        .strip_prefix("HTTP/")
        .and_then(|reste| reste.split_whitespace().nth(1))
        .and_then(|c| c.parse::<u16>().ok())
        .ok_or_else(|| invalide(format!("ligne d'état illisible : {etat}")))?;
    let (mut longueur, mut morceaux) = (None, false);
    loop {
        let ligne = lire_ligne(r)?;
        if ligne.is_empty() {
            break;
        }
        let Some((nom, valeur)) = ligne.split_once(':') else { continue };
        let (nom, valeur) = (nom.trim(), valeur.trim());
        if nom.eq_ignore_ascii_case("content-length") {
            longueur = Some(valeur.parse::<usize>().map_err(|_| invalide(format!("longueur illisible : {valeur}")))?);
        } else if nom.eq_ignore_ascii_case("transfer-encoding") {
            morceaux = valeur.to_ascii_lowercase().contains("chunked");
        }
    }
    let mut corps = Vec::new();
    if morceaux {
        loop {
            let ligne = lire_ligne(r)?;
            let taille = ligne.split(';').next().unwrap_or("").trim();
            let taille =
                usize::from_str_radix(taille, 16).map_err(|_| invalide(format!("morceau illisible : {ligne}")))?;
            if taille == 0 {
                // Les en-têtes de fin, s'il y en a, jusqu'à la ligne vide (ou la fermeture).
                while lire_ligne(r).is_ok_and(|l| !l.is_empty()) {}
                break;
            }
            if corps.len() + taille > CORPS_MAX {
                return Err(invalide("réponse trop longue".into()));
            }
            let debut = corps.len();
            corps.resize(debut + taille, 0);
            r.read_exact(&mut corps[debut..])?;
            if !lire_ligne(r)?.is_empty() {
                return Err(invalide("morceau mal terminé".into()));
            }
        }
    } else if let Some(n) = longueur {
        if n > CORPS_MAX {
            return Err(invalide("réponse trop longue".into()));
        }
        corps.resize(n, 0);
        r.read_exact(&mut corps)?;
    } else {
        r.take(CORPS_MAX as u64).read_to_end(&mut corps)?;
    }
    Ok((code, corps))
}

/// Une ligne de la réponse, sans sa fin (CRLF ou LF).
fn lire_ligne<R: BufRead>(r: &mut R) -> io::Result<String> {
    let mut ligne = Vec::new();
    r.by_ref().take(LIGNE_MAX).read_until(b'\n', &mut ligne)?;
    if ligne.last() != Some(&b'\n') {
        return Err(if ligne.len() as u64 >= LIGNE_MAX {
            invalide("ligne trop longue".into())
        } else {
            io::Error::new(io::ErrorKind::UnexpectedEof, "réponse tronquée")
        });
    }
    ligne.pop();
    if ligne.last() == Some(&b'\r') {
        ligne.pop();
    }
    Ok(String::from_utf8_lossy(&ligne).into_owned())
}

fn invalide(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

/// Le nom court d'un modèle : « ggml-small-q5_1.bin » donne « small ».
fn nom_du_modele(chemin: &Path) -> String {
    let tige = chemin.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tige = tige.strip_prefix("ggml-").unwrap_or(tige.as_str());
    let nom: Vec<&str> = tige.split('-').take_while(|m| !est_une_quantification(m)).collect();
    if nom.is_empty() { tige.to_string() } else { nom.join("-") }
}

/// « q5_1 », « q8_0 », « f16 » : la précision d'un modèle, pas son nom.
fn est_une_quantification(morceau: &str) -> bool {
    let mut c = morceau.chars();
    matches!(morceau, "f16" | "f32") || (c.next() == Some('q') && c.next().is_some_and(|x| x.is_ascii_digit()))
}

/// Des secondes à la française : « 2,3 ».
fn en_francais(secondes: f32) -> String {
    format!("{secondes:.1}").replace('.', ",")
}

/// Le nombre de fils de calcul de Whisper : un cœur de moins que le poste en a, au plus 8. Les fils de ggml s'attendent
/// en tournant à vide : sur tous les cœurs d'un poste occupé (Edge, Outlook, la synchronisation des fichiers, Claude Code), un fil interrompu
/// fait patienter les autres.
pub fn fils_de_calcul() -> i32 {
    let coeurs = thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    coeurs.saturating_sub(1).clamp(1, 8) as i32
}

/// Règle le fil courant pour arrondir à zéro les nombres dénormaux (les bits FTZ et DAZ du registre MXCSR) : pour les
/// essais (sur un processeur de 2012, sans effet mesurable).
pub fn arrondir_les_denormaux() {
    #[cfg(target_arch = "x86_64")]
    {
        let mut csr: u32 = 0;
        // SAFETY : stmxcsr et ldmxcsr lisent et écrivent le registre de contrôle SSE du fil courant ; seuls les bits
        // FTZ (15) et DAZ (6) changent.
        unsafe {
            std::arch::asm!("stmxcsr [{}]", in(reg) &raw mut csr, options(nostack));
            csr |= 0x8040;
            std::arch::asm!("ldmxcsr [{}]", in(reg) &raw const csr, options(nostack, readonly));
        }
    }
}

/// Charge un modèle ; rend l'état de transcription et la durée du chargement.
pub fn charger(modele: &Path) -> Result<(WhisperState, f32), String> {
    if !modele.is_file() {
        return Err(format!("le modèle de Whisper est introuvable : {}", modele.display()));
    }
    let t0 = Instant::now();
    let contexte = WhisperContext::new_with_params(modele, WhisperContextParameters::default())
        .map_err(|e| format!("le modèle de Whisper ne se charge pas : {e}"))?;
    let etat = contexte.create_state().map_err(|e| format!("Whisper ne démarre pas : {e}"))?;
    Ok((etat, t0.elapsed().as_secs_f32()))
}

/// La fenêtre d'encodage d'une phrase : sa durée réelle (50 trames par seconde), plus une marge, au lieu des 30 s
/// fixes de Whisper. L'encodeur est le gros du calcul et croît plus vite que la fenêtre ; une phrase de dix secondes
/// s'encode ainsi trois à six fois plus vite, le silence de remplissage en moins.
pub fn fenetre_d_encodage(echantillons: usize) -> i32 {
    let trames = (echantillons as f64 / CADENCE as f64 * 50.0).ceil() as i32;
    (trames + 64).clamp(256, 1500)
}

/// La fenêtre voulue pour une phrase : `EVE_FENETRE` pour les essais (0 : les 30 s pleines), sinon la sienne, jamais
/// sous `plancher` (celui du modèle).
fn fenetre_voulue(echantillons: usize, plancher: i32) -> i32 {
    std::env::var("EVE_FENETRE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| fenetre_d_encodage(echantillons).max(plancher))
}

/// La plus petite fenêtre qu'un modèle tient sur la carte. Les grands (large-v3, large-v3-turbo) n'ont appris que des
/// fenêtres de 30 s : sous ce plancher, une phrase courte les fait boucler ou inventer, et leurs essais à température
/// plus haute recommencent (mesuré le 2026-10-23, voir `PLANCHER_DES_GRANDS`) ; small et base tiennent la fenêtre de la
/// phrase.
fn plancher_de_fenetre(modele: &Path) -> i32 {
    if nom_du_modele(modele).starts_with("large") { PLANCHER_DES_GRANDS } else { 0 }
}

/// Whisper veut au moins une seconde : un mot bref se complète de silence.
fn au_moins_une_seconde(audio: &[f32]) -> Cow<'_, [f32]> {
    const MIN: usize = CADENCE as usize + 1_600;
    if audio.len() >= MIN {
        return Cow::Borrowed(audio);
    }
    let mut a = audio.to_vec();
    a.resize(MIN, 0.0);
    Cow::Owned(a)
}

/// Une phrase (16 kHz, mono) en texte, sur le processeur.
pub fn transcrire(etat: &mut WhisperState, audio: &[f32], amorce: &str, fils: i32) -> String {
    let mut p = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    let fenetre = fenetre_voulue(audio.len(), 0);
    if fenetre > 0 {
        p.set_audio_ctx(fenetre);
    }
    p.set_language(Some("fr"));
    p.set_translate(false);
    p.set_n_threads(fils);
    p.set_no_context(true);
    p.set_no_timestamps(true);
    p.set_suppress_blank(true);
    p.set_suppress_nst(true);
    p.set_print_special(false);
    p.set_print_progress(false);
    p.set_print_realtime(false);
    p.set_print_timestamps(false);
    if !amorce.is_empty() {
        p.set_initial_prompt(amorce);
    }
    let a = au_moins_une_seconde(audio);
    match etat.full(p, &a) {
        Ok(()) => (0..etat.full_n_segments())
            .filter_map(|i| etat.get_segment(i))
            .filter_map(|s| s.to_str_lossy().ok().map(|t| t.into_owned()))
            .collect::<Vec<_>>()
            .join(" ")
            .trim()
            .to_string(),
        Err(e) => {
            journal::ecrire(&format!("oreille : {e}"));
            String::new()
        }
    }
}

#[cfg(test)]
mod essais {
    use super::*;

    #[test]
    fn la_fenetre_de_la_carte_est_un_multiple_de_256() {
        for (fenetre, attendu) in [(256, 256), (257, 512), (604, 768), (1024, 1024), (1100, 1280), (1280, 1280)] {
            assert_eq!(audio_ctx_carte(fenetre), attendu, "{fenetre}");
        }
        for fenetre in [1281, 1500, 0, -3] {
            assert_eq!(audio_ctx_carte(fenetre), 0, "{fenetre}");
        }
        // La phrase des mesures (10,8 s), le début cherché pour « Eve » (2,5 s), 5,1 s, puis 25 s : la fenêtre pleine.
        assert_eq!(audio_ctx_carte(fenetre_d_encodage(172_511)), 768);
        assert_eq!(audio_ctx_carte(fenetre_d_encodage(DEBUT_EVEIL)), 256);
        assert_eq!(audio_ctx_carte(fenetre_d_encodage(81_600)), 512);
        assert_eq!(audio_ctx_carte(fenetre_d_encodage(400_000)), 0);
    }

    #[test]
    fn les_grands_modeles_gardent_une_fenetre_de_768() {
        let turbo = plancher_de_fenetre(Path::new(r"C:\modeles\ggml-large-v3-turbo-q5_0.bin"));
        assert_eq!(turbo, 768);
        assert_eq!(plancher_de_fenetre(Path::new("ggml-large-v3-q5_0.bin")), 768);
        assert_eq!(plancher_de_fenetre(Path::new("ggml-small-q5_1.bin")), 0, "small tient la fenêtre de sa phrase");
        assert_eq!(plancher_de_fenetre(Path::new("ggml-base-q5_1.bin")), 0);
        // « Deux, OK. » (2,5 s) : 256 trames pour small, 768 pour turbo ; une phrase de 20 s garde la sienne.
        if std::env::var_os("EVE_FENETRE").is_none() {
            assert_eq!(audio_ctx_carte(fenetre_voulue(40_000, 0)), 256);
            assert_eq!(audio_ctx_carte(fenetre_voulue(40_000, turbo)), 768);
            assert_eq!(audio_ctx_carte(fenetre_voulue(320_000, turbo)), 1280);
        }
    }

    #[test]
    fn le_wav_a_l_en_tete_attendue() {
        let audio = [0.0, 0.5, -0.5, 1.0, -1.0, 2.0];
        let w = wav_16k(&audio);
        let u16_a = |i: usize| u16::from_le_bytes([w[i], w[i + 1]]);
        let u32_a = |i: usize| u32::from_le_bytes([w[i], w[i + 1], w[i + 2], w[i + 3]]);
        assert_eq!(w.len(), 44 + 12);
        assert_eq!(&w[0..4], b"RIFF");
        assert_eq!(u32_a(4), 36 + 12);
        assert_eq!(&w[8..16], b"WAVEfmt ");
        assert_eq!((u32_a(16), u16_a(20), u16_a(22)), (16, 1, 1));
        assert_eq!((u32_a(24), u32_a(28), u16_a(32), u16_a(34)), (16_000, 32_000, 2, 16));
        assert_eq!(&w[36..40], b"data");
        assert_eq!(u32_a(40), 12);
        let echantillons: Vec<i16> = w[44..].as_chunks::<2>().0.iter().map(|b| i16::from_le_bytes(*b)).collect();
        assert_eq!(echantillons, [0, 16_384, -16_384, 32_767, -32_768, 32_767]);
        // Le lecteur des essais le relit tel quel.
        let fichier = std::env::temp_dir().join(format!("eve-essai-wav-{}.wav", std::process::id()));
        std::fs::write(&fichier, &w).unwrap();
        let relu = crate::micro::lire_wav(&fichier);
        let _ = std::fs::remove_file(&fichier);
        let relu = relu.unwrap();
        assert_eq!(relu.len(), audio.len());
        assert!((relu[1] - 0.5).abs() < 1e-4 && (relu[2] + 0.5).abs() < 1e-4, "{relu:?}");
    }

    #[test]
    fn le_formulaire_multipart() {
        let c = corps_multipart("F", b"RIFF..", &[("language", "fr"), ("prompt", "Eve, Entrepreneur Général")]);
        let attendu = "--F\r\nContent-Disposition: form-data; name=\"file\"; filename=\"phrase.wav\"\r\n\
                       Content-Type: audio/wav\r\n\r\nRIFF..\r\n\
                       --F\r\nContent-Disposition: form-data; name=\"language\"\r\n\r\nfr\r\n\
                       --F\r\nContent-Disposition: form-data; name=\"prompt\"\r\n\r\nEve, Entrepreneur Général\r\n\
                       --F--\r\n";
        assert_eq!(String::from_utf8(c).unwrap(), attendu);
    }

    #[test]
    fn la_tete_de_requete() {
        assert_eq!(
            tete_de_requete("GET", "/health", 8178, None, 0),
            "GET /health HTTP/1.1\r\nHost: 127.0.0.1:8178\r\nConnection: close\r\n\r\n"
        );
        assert_eq!(
            tete_de_requete("POST", "/inference", 8179, Some("multipart/form-data; boundary=F"), 42),
            "POST /inference HTTP/1.1\r\nHost: 127.0.0.1:8179\r\nContent-Type: multipart/form-data; boundary=F\r\n\
             Content-Length: 42\r\nConnection: close\r\n\r\n"
        );
    }

    #[test]
    fn une_reponse_a_longueur_fixe() {
        let r = b"HTTP/1.1 200 OK\r\nServer: whisper.cpp\r\nContent-Type: application/json\r\ncontent-length: 15\r\n\
                  Keep-Alive: timeout=5, max=100\r\n\r\n{\"status\":\"ok\"}de trop";
        assert_eq!(lire_reponse(&mut &r[..]).unwrap(), (200, b"{\"status\":\"ok\"}".to_vec()));
    }

    #[test]
    fn une_reponse_en_morceaux() {
        let r =
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: Chunked\r\n\r\n9\r\n{\"text\":\"\r\nb;ext=1\r\n Bonjour.\\n\r\n\
                  2\r\n\"}\r\n0\r\nX-Fin: 1\r\n\r\n";
        let (code, corps) = lire_reponse(&mut &r[..]).unwrap();
        assert_eq!(code, 200);
        assert_eq!(String::from_utf8_lossy(&corps), "{\"text\":\" Bonjour.\\n\"}");
        assert_eq!(texte_de_la_reponse(&corps).unwrap(), "Bonjour.");
    }

    #[test]
    fn une_reponse_jusqu_a_la_fermeture_et_les_reponses_tronquees() {
        let r = b"HTTP/1.1 503 Service Unavailable\r\n\r\n{\"status\":\"loading model\"}";
        assert_eq!(lire_reponse(&mut &r[..]).unwrap(), (503, b"{\"status\":\"loading model\"}".to_vec()));
        // Tronquée : une erreur, jamais un texte à moitié lu.
        assert!(lire_reponse(&mut &b"HTTP/1.1 200 OK\r\nContent-Length: 50\r\n\r\n{\"text\""[..]).is_err());
        assert!(lire_reponse(&mut &b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nabc"[..]).is_err());
        assert!(lire_reponse(&mut &b"HTTP/1.1 200 OK\r\nContent-Le"[..]).is_err());
        assert!(lire_reponse(&mut &b"bonjour\r\n\r\n"[..]).is_err());
        assert!(lire_reponse(&mut &b""[..]).is_err());
    }

    #[test]
    fn le_texte_de_la_reponse() {
        let r = br#"{"text":" Eve, quelle heure est-il ?\n Merci.\n"}"#;
        assert_eq!(texte_de_la_reponse(r).unwrap(), "Eve, quelle heure est-il ? Merci.");
        assert_eq!(texte_de_la_reponse(br#"{"text":" ...\n"}"#).unwrap(), "...");
        assert!(texte_de_la_reponse(br#"{"error":"failed to read audio data"}"#).is_err());
        assert!(texte_de_la_reponse(b"<html>").is_err());
    }

    #[test]
    fn la_premiere_ecoute_suffit_quand_elle_a_tout_entendu() {
        // Sur la carte (le fidèle cherche « Eve ») : la phrase entière ne se transcrit pas deux fois.
        assert!(premiere_ecoute_suffit(true, true, "quelle heure est-il ?"));
        assert!(!premiere_ecoute_suffit(false, true, "quelle heure"));
        // Sur le processeur (le rapide cherche « Eve ») : « Eve » seul et un arrêt partent tout de suite.
        assert!(premiere_ecoute_suffit(true, false, ""));
        assert!(premiere_ecoute_suffit(true, false, "stop."));
        assert!(premiere_ecoute_suffit(true, false, "Arrête !"));
        assert!(!premiere_ecoute_suffit(true, false, "quelle heure est-il ?"));
        // Une phrase plus longue que le début : « arrête de chercher… » est une demande, pas un arrêt.
        assert!(!premiere_ecoute_suffit(false, false, "arrête de chercher"));
    }

    #[test]
    fn le_nom_du_modele() {
        for (fichier, nom) in [
            ("m/ggml-small-q5_1.bin", "small"),
            ("m/ggml-base.bin", "base"),
            ("m/ggml-medium-q8_0.bin", "medium"),
            ("m/ggml-large-v3-turbo-q5_0.bin", "large-v3-turbo"),
            ("m/ggml-tiny-f16.bin", "tiny"),
            ("m/mon-modele.bin", "mon-modele"),
        ] {
            assert_eq!(nom_du_modele(Path::new(fichier)), nom);
        }
        assert_eq!(en_francais(2.345), "2,3");
    }

    /// Un faux serveur sur 127.0.0.1 : la requête part entière (en-têtes, formulaire, WAV), la réponse en morceaux se
    /// lit.
    #[test]
    fn une_transcription_passe_par_le_formulaire() {
        let ecoute = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = ecoute.local_addr().unwrap().port();
        let serveur = thread::spawn(move || {
            let (mut flux, _) = ecoute.accept().unwrap();
            let mut lecteur = BufReader::new(flux.try_clone().unwrap());
            let mut tete = String::new();
            loop {
                let mut ligne = String::new();
                lecteur.read_line(&mut ligne).unwrap();
                if ligne == "\r\n" {
                    break;
                }
                tete.push_str(&ligne);
            }
            let longueur = tete.lines().find_map(|l| l.strip_prefix("Content-Length: ")).unwrap();
            let mut corps = vec![0; longueur.trim().parse().unwrap()];
            lecteur.read_exact(&mut corps).unwrap();
            let json = br#"{"text":" Bonjour Martin.\n"}"#;
            let mut reponse = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n".to_vec();
            for morceau in [&json[..17], &json[17..]] {
                reponse.extend_from_slice(format!("{:x}\r\n", morceau.len()).as_bytes());
                reponse.extend_from_slice(morceau);
                reponse.extend_from_slice(b"\r\n");
            }
            reponse.extend_from_slice(b"0\r\n\r\n");
            flux.write_all(&reponse).unwrap();
            (tete, corps)
        });
        // La phrase des mesures (10,8 s) : la fenêtre de 604 trames s'arrondit à 768.
        let audio = vec![0.1; 172_511];
        let texte = inference(port, &audio, "Eve, Toitures Exemple inc.", 0).unwrap();
        let (tete, corps) = serveur.join().unwrap();
        assert_eq!(texte, "Bonjour Martin.");
        assert!(tete.starts_with("POST /inference HTTP/1.1\r\n"), "{tete}");
        assert!(tete.contains("Connection: close\r\n"), "{tete}");
        assert!(tete.contains(&format!("Content-Type: multipart/form-data; boundary={FRONTIERE}\r\n")), "{tete}");
        let champs = [
            ("language", "fr"),
            ("audio_ctx", "768"),
            ("best_of", "1"),
            ("beam_size", "1"),
            ("no_timestamps", "true"),
            ("suppress_nst", "true"),
            ("temperature", "0.0"),
            ("temperature_inc", "0.2"),
            ("response_format", "json"),
            ("prompt", "Eve, Toitures Exemple inc."),
        ];
        assert!(
            corps == corps_multipart(FRONTIERE, &wav_16k(&audio), &champs),
            "le formulaire n'est pas celui attendu"
        );
    }

    /// Un serveur qui accepte et ne répond jamais : la requête rend la main à son échéance.
    #[test]
    fn un_serveur_muet_ne_bloque_pas() {
        let ecoute = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = ecoute.local_addr().unwrap().port();
        let muet = thread::spawn(move || {
            let connexion = ecoute.accept();
            thread::sleep(Duration::from_secs(1));
            drop(connexion);
        });
        let t0 = Instant::now();
        let r = requete(port, "GET", "/health", None, &[], Duration::from_millis(300), DELAI_SONDE);
        let ecoule = t0.elapsed();
        assert!(r.is_err());
        assert!(ecoule >= Duration::from_millis(250) && ecoule < Duration::from_millis(900), "{ecoule:?}");
        muet.join().unwrap();
    }

    // ---- Sur le poste : le serveur de `whisper-gpu`, les modèles et les sons des essais ------------------------------

    fn son_d_essai(nom: &str) -> Vec<f32> {
        let chemin = dossier_local().join("essais").join(nom);
        crate::micro::lire_wav(&chemin).unwrap_or_else(|e| panic!("{e}"))
    }

    fn secondes(audio: &[f32]) -> f32 {
        audio.len() as f32 / CADENCE as f32
    }

    /// Les annonces de l'oreille arrivées jusqu'ici.
    fn annonces(rx: &mpsc::Receiver<Evt>) -> Vec<String> {
        rx.try_iter()
            .filter_map(|evt| match evt {
                Evt::Oreille(Ok(d)) => Some(d),
                Evt::Oreille(Err(e)) => Some(format!("ERREUR : {e}")),
                Evt::EveilEntendu => Some("« Eve » entendu".into()),
                Evt::Eveil { demande, .. } => Some(format!("demande : « {demande} »")),
                _ => None,
            })
            .collect()
    }

    /// `cargo test --release -- --ignored --nocapture la_carte_graphique_transcrit`
    #[test]
    #[ignore]
    fn la_carte_graphique_transcrit_voix2() {
        let local = dossier_local();
        let audio = son_d_essai("voix2.wav");
        let mut carte = Carte::lancer(
            &local.join("whisper-gpu").join("whisper-server.exe"),
            &local.join("modeles").join("ggml-small-q5_1.bin"),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        println!("serveur : {} (pid {})", carte.resume(), carte.enfant.id());
        for i in 1..=3 {
            let t0 = Instant::now();
            let texte = carte.transcrire(&audio, crate::reglages::VOCABULAIRE).unwrap_or_else(|e| panic!("{e}"));
            println!("voix2 ({:.1} s), essai {i} : {:.2} s — {texte}", secondes(&audio), t0.elapsed().as_secs_f32());
            assert!(!texte.is_empty(), "{texte}");
        }
        let amorce = format!("Eve, {}", crate::reglages::VOCABULAIRE);
        let t0 = Instant::now();
        let debut = carte.transcrire(&audio[..DEBUT_EVEIL], &amorce).unwrap_or_else(|e| panic!("{e}"));
        println!("début (2,5 s) pour « Eve » : {:.2} s — {debut}", t0.elapsed().as_secs_f32());
        carte.arreter();
        assert!(carte.enfant.try_wait().unwrap().is_some(), "le serveur tourne encore");
    }

    /// Le mot d'éveil sur la carte : « Eve, stop. » et « Eve ? » tiennent dans les 2,5 premières secondes (une seule
    /// transcription), « Eve, quelles sont les factures… » non (une seconde, sur la phrase entière) ; voix2 n'appelle
    /// pas Eve. Les sons « eve-*.wav » viennent de Piper (la voix Siwis).
    /// `cargo test --release -- --ignored --nocapture l_eveil_sur_la_carte`
    #[test]
    #[ignore]
    fn l_eveil_sur_la_carte_graphique() {
        let local = dossier_local();
        let (tx, rx) = mpsc::channel();
        let mut oreille = Oreille::ouvrir(
            local.join("modeles").join("ggml-small-q5_1.bin"),
            local.join("modeles").join("ggml-small-q5_1.bin"),
            local.join("modeles").join("ggml-base-q5_1.bin"),
            tx,
        );
        println!("{:?}", annonces(&rx));
        assert!(matches!(oreille.moteur, Moteur::Carte(_)), "pas sur la carte graphique");
        let amorce = format!("Eve, {}", crate::reglages::VOCABULAIRE);
        for nom in ["eve-stop.wav", "eve-seul.wav", "eve-demande.wav", "voix2.wav"] {
            let audio = son_d_essai(nom);
            let t0 = Instant::now();
            oreille.eveil(&audio, &amorce, t0);
            println!("{nom} ({:.1} s) : {:.2} s — {:?}", secondes(&audio), t0.elapsed().as_secs_f32(), annonces(&rx));
        }
    }

    /// Le serveur tué en pleine séance est relancé, et la phrase transcrite quand même ; retombé aussitôt, il cède au
    /// processeur pour la suite — sans perdre la phrase.
    /// `cargo test --release -- --ignored --nocapture la_carte_qui_tombe`
    #[test]
    #[ignore]
    fn la_carte_qui_tombe_cede_au_processeur() {
        whisper_rs::install_logging_hooks();
        let local = dossier_local();
        let (tx, rx) = mpsc::channel();
        let mut oreille = Oreille::ouvrir(
            local.join("modeles").join("ggml-small-q5_1.bin"),
            local.join("modeles").join("ggml-small-q5_1.bin"),
            local.join("modeles").join("ggml-base-q5_1.bin"),
            tx,
        );
        println!("{:?}", annonces(&rx));
        let audio = son_d_essai("voix2.wav");
        let phrase = &audio[..CADENCE as usize * 5];
        let tuer = |oreille: &mut Oreille| {
            let Moteur::Carte(carte) = &mut oreille.moteur else { panic!("pas sur la carte graphique") };
            let pid = carte.enfant.id();
            carte.enfant.kill().unwrap();
            carte.enfant.wait().unwrap();
            pid
        };
        let pid = tuer(&mut oreille);
        let t0 = Instant::now();
        let texte = oreille.entendre(phrase, crate::reglages::VOCABULAIRE, Modele::Fidele);
        println!("serveur {pid} tué ; relancé et transcrit en {:.2} s — {texte}", t0.elapsed().as_secs_f32());
        assert!(!texte.is_empty());
        let pid = tuer(&mut oreille);
        let t0 = Instant::now();
        let texte = oreille.entendre(phrase, crate::reglages::VOCABULAIRE, Modele::Fidele);
        println!("serveur {pid} retué ; sur le processeur en {:.2} s — {texte}", t0.elapsed().as_secs_f32());
        assert!(!texte.is_empty());
        assert!(matches!(oreille.moteur, Moteur::Processeur(_)), "pas passé au processeur");
        println!("{:?}", annonces(&rx));
    }

    /// Le repli seul, pour comparer : le fidèle sur le processeur, la phrase entière et ses 5 premières secondes
    /// (`EVE_FENETRE` change la fenêtre). `cargo test --release -- --ignored --nocapture le_processeur_transcrit`
    #[test]
    #[ignore]
    fn le_processeur_transcrit_voix2() {
        whisper_rs::install_logging_hooks();
        let (mut etat, duree) = charger(&dossier_local().join("modeles").join("ggml-small-q5_1.bin")).unwrap();
        println!("small chargé en {duree:.1} s ; {} fils", fils_de_calcul());
        let audio = son_d_essai("voix2.wav");
        for phrase in [&audio[..], &audio[..CADENCE as usize * 5]] {
            let t0 = Instant::now();
            let texte = transcrire(&mut etat, phrase, crate::reglages::VOCABULAIRE, fils_de_calcul());
            let (s, f) = (secondes(phrase), fenetre_voulue(phrase.len(), 0));
            println!("{s:.1} s, fenêtre {f} : {:.2} s — {texte}", t0.elapsed().as_secs_f32());
        }
    }
}
