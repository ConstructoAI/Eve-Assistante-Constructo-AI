//! Le téléphone : l'appli Android d'Eve (`mobile\` du dépôt) est une télécommande de cette Eve — une fenêtre, un micro
//! et un haut-parleur de plus. Le cerveau reste ici : Claude Code, les fichiers, l'ERP, l'oreille et la voix. Ce module
//! est ce que l'Eve de l'ordinateur en tient : les téléphones jumelés, leur serveur, et ce qui passe entre les deux.
//!
//! **Le jumelage.** « Jumeler un téléphone », dans les réglages de la fenêtre, ouvre une offre de cinq minutes : un code
//! carré qui porte un lien `eve://jumeler?…` — les adresses de l'ordinateur, son port, l'empreinte de son certificat et
//! un code de 256 bits, à usage unique. Le téléphone le scanne et se présente (`POST /jumeler`) avec ce code et une clé
//! à lui, de 256 bits, qu'il a tirée lui-même : le code carré ne l'a jamais montrée, et Eve n'en garde que le SHA-256.
//! Une offre à la fois ; elle se ferme au premier jumelage, quand on ferme le panneau, ou au bout de cinq minutes.
//!
//! **Les téléphones jumelés.** `telephones.cle` du dossier local, chiffré pour la session de Windows (DPAPI) : un fichier
//! `.cle`, que la garde refuse à Claude — une page ou un courriel piégés ne lui feraient pas glisser un téléphone de plus.
//! Un téléphone se retire dans les réglages ; sa clé ne vaut alors plus rien.
//!
//! **Le serveur.** Son port (`port_telephone` ; d'office, celui de la fenêtre plus mille), sur toutes les adresses du
//! poste : le Wi-Fi ou le câble de la maison, et Tailscale s'il est là. Il n'écoute que tant qu'un téléphone est jumelé ou
//! qu'une offre est ouverte : sans téléphone, Eve n'écoute pas le réseau. Tout passe en TLS (rustls), sous un certificat
//! qu'Eve tire elle-même au premier jumelage (`telephone-tls.cle`, chiffré de même) : le téléphone n'accepte que lui, par
//! son empreinte. Chaque requête porte la clé du téléphone ; vingt refus en dix minutes ferment le serveur à tous pour
//! dix minutes.
//!
//! **Ce qui passe.** L'état de la fenêtre (`GET /flux`, comme la page) allégé — ni les réglages, ni les cartes épinglées,
//! ni la comptabilité —, la conversation seulement quand elle change ; les gestes de la conversation (`POST /action` :
//! écrire, confirmer ou annuler, arrêter, se taire, une nouvelle conversation, retirer une pièce ; rien d'autre) ; la voix
//! de la personne (`POST /son`, un WAV que l'oreille de l'ordinateur transcrit) ; ses photos (`POST /piece`, jointes comme
//! un fichier déposé) ; le portrait d'Eve (`GET /portrait`) ; et, quand la demande vient du téléphone, la voix d'Eve —
//! Piper, faite ici —, phrase par phrase, dans le flux ([`Renvoi`]).

use std::collections::VecDeque;
use std::fs;
use std::io::{self, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::chef::Evt;
use crate::connexion::{self, b64url, b64url_decoder};
use crate::ecran::{self, Requete};
use crate::journal;
use crate::reglages::ecrire_d_un_geste;

/// Les téléphones jumelés ; le certificat du serveur et sa clé. Chiffrés tous deux pour la session de Windows.
const REGISTRE: &str = "telephones.cle";
const IDENTITE: &str = "telephone-tls.cle";
/// L'« entropie » de DPAPI pour ces deux fichiers : un autre programme de la session doit la connaître pour les lire.
const ENTROPIE: &[u8] = b"Eve/telephones/1";
/// Une offre de jumelage vaut ce temps.
const OFFRE_DUREE: Duration = Duration::from_secs(5 * 60);
/// La voix de la personne : 45 s au plus (comme l'écoute à la touche de l'ordinateur), d'ordinaire en WAV de 16 kHz,
/// 16 bits, mono — 1,4 Mo.
const SON_MAX: u64 = 2 * 1024 * 1024;
/// Le portrait d'Eve que le téléphone montre : 25 Mo au plus (celui du kit fait un mégaoctet).
const PORTRAIT_MAX: u64 = 25 * 1024 * 1024;
/// Les refus : au-delà de tant en si peu de temps, le serveur se ferme à tous pour ce temps.
const REFUS_MAX: usize = 20;
const REFUS_FENETRE: Duration = Duration::from_secs(10 * 60);
const FERME_POUR: Duration = Duration::from_secs(10 * 60);
/// Le battement du flux : un téléphone parti se voit au plus tard au battement suivant, et une liaison en 4G ne
/// s'endort pas.
const BATTEMENT: Duration = Duration::from_secs(15);
/// La conversation qu'envoie le flux : ses dernières lignes (le fil de la fenêtre en garde quatre-vingts).
const FIL_MAX: usize = 60;
/// Ce que met une phrase de la voix d'Eve à s'entendre sur le téléphone : l'envoi, puis l'ouverture du son. Une
/// estimation : le téléphone ne dit pas quand il joue.
pub const ATTENTE_DU_TELEPHONE: Duration = Duration::from_millis(250);
const TEXTE: &str = "text/plain; charset=utf-8";
const JSON: &str = "application/json; charset=utf-8";

/// Les gestes que le téléphone peut faire : ceux de la conversation, rien d'autre (ni un réglage, ni la mise à jour, ni
/// un jumelage, ni « quitter »).
const GESTES: [&str; 7] = ["texte", "confirmer", "annuler", "arreter", "taire", "nouvelle", "piece_retirer"];
/// Ce que le téléphone voit de l'état de la fenêtre.
const CHAMPS: [&str; 15] = [
    "etat", "libelle", "aide", "emotion", "qui", "nom", "profil", "accord", "activite", "pieces", "a_corriger",
    "fil_rev", "fil", "en_cours", "version",
];

/// Ce que le serveur des téléphones dit au chef.
pub enum Evenement {
    /// Un téléphone vient de se jumeler.
    Jumele {
        nom: String,
        qui: String,
    },
    /// Un téléphone s'est branché sur le flux, ou en est parti.
    Branche {
        telephone: String,
    },
    Parti {
        telephone: String,
    },
    /// Un geste de la conversation, fait sur le téléphone ([`GESTES`]).
    Geste {
        telephone: String,
        action: String,
        valeur: Value,
    },
    /// La voix de la personne, tenue sur le téléphone : à 16 kHz mono, reçue à `fin`.
    Son {
        telephone: String,
        audio: Vec<f32>,
        fin: Instant,
    },
    /// Trop de refus : le serveur se ferme à tous pour dix minutes.
    Ferme,
}

/// Un téléphone jumelé.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Telephone {
    /// Son numéro : seize chiffres hexadécimaux, tirés au jumelage.
    pub id: String,
    /// Son nom, tel qu'il se donne (« Pixel 8 ») : celui des réglages.
    pub nom: String,
    /// La personne devant le poste quand il s'est jumelé : c'est elle qui lui parle.
    pub qui: String,
    /// Quand il s'est jumelé.
    pub jumele: String,
    /// Le SHA-256 de sa clé, en hexadécimal : la clé elle-même n'est que sur le téléphone.
    empreinte: String,
}

/// `telephones.cle`, tel qu'Eve le connaît.
struct Registre {
    chemin: PathBuf,
    telephones: Vec<Telephone>,
    /// Le fichier est là mais ne se lit pas (une autre session de Windows, un fichier abîmé) : Eve n'y écrit rien.
    illisible: bool,
}

impl Registre {
    fn ouvrir(local: &Path) -> Registre {
        let chemin = local.join(REGISTRE);
        let chiffre = match fs::read(&chemin) {
            Ok(o) => o,
            Err(_) => return Registre { chemin, telephones: Vec::new(), illisible: false },
        };
        let lus = connexion::proteger(&chiffre, false, ENTROPIE, "")
            .ok()
            .and_then(|clair| serde_json::from_slice::<Value>(&clair).ok())
            .and_then(|v| serde_json::from_value::<Vec<Telephone>>(v["telephones"].clone()).ok());
        match lus {
            Some(telephones) => Registre { chemin, telephones, illisible: false },
            None => {
                journal::ecrire(&format!("téléphone : {REGISTRE} ne se lit pas ici ; Eve n'y écrira rien"));
                Registre { chemin, telephones: Vec::new(), illisible: true }
            }
        }
    }

    fn ecrire(&self) -> Result<(), String> {
        if self.illisible {
            return Err(format!("{REGISTRE} ne se lit pas ici : Eve n'y écrit rien"));
        }
        let clair = json!({ "telephones": self.telephones }).to_string();
        let chiffre = connexion::proteger(clair.as_bytes(), true, ENTROPIE, "Eve - téléphones jumelés")?;
        ecrire_d_un_geste(&self.chemin, &chiffre).map_err(|e| format!("{REGISTRE} ne s'écrit pas ({e})"))
    }

    /// Le téléphone de cette clé, s'il est jumelé (les empreintes comparées en temps constant).
    fn reconnaitre(&self, cle: &[u8]) -> Option<Telephone> {
        let empreinte = hex(&connexion::sha256(cle).ok()?);
        self.telephones.iter().find(|t| egaux(t.empreinte.as_bytes(), empreinte.as_bytes())).cloned()
    }
}

/// Le certificat du serveur, et sa configuration TLS.
struct Identite {
    config: Arc<rustls::ServerConfig>,
    /// Le SHA-256 du certificat, en base64url : ce que le téléphone accepte, et rien d'autre.
    empreinte: String,
}

/// Le certificat de ce dossier local, tiré au premier besoin (une clé P-256, qu'aucune autorité ne signe : le téléphone
/// le reconnaît à son empreinte, que lui a donnée le code carré).
fn identite(local: &Path) -> Result<Identite, String> {
    let chemin = local.join(IDENTITE);
    let (certificat, cle) = match fs::read(&chemin) {
        Ok(chiffre) => {
            let clair = connexion::proteger(&chiffre, false, ENTROPIE, "")?;
            let v: Value = serde_json::from_slice(&clair).map_err(|e| format!("{IDENTITE} : {e}"))?;
            let lire = |nom: &str| {
                v[nom].as_str().and_then(b64url_decoder).ok_or_else(|| format!("{IDENTITE} : « {nom} » illisible"))
            };
            (lire("certificat")?, lire("cle")?)
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            let neuf = rcgen::generate_simple_self_signed(vec!["eve".to_string()])
                .map_err(|e| format!("le certificat ne se fait pas ({e})"))?;
            let (certificat, cle) = (neuf.cert.der().to_vec(), neuf.signing_key.serialize_der());
            let clair = json!({ "certificat": b64url(&certificat), "cle": b64url(&cle) }).to_string();
            let chiffre = connexion::proteger(clair.as_bytes(), true, ENTROPIE, "Eve - certificat des téléphones")?;
            ecrire_d_un_geste(&chemin, &chiffre).map_err(|e| format!("{IDENTITE} ne s'écrit pas ({e})"))?;
            journal::ecrire("téléphone : un certificat neuf pour le serveur");
            (certificat, cle)
        }
        Err(e) => return Err(format!("{IDENTITE} ne se lit pas ({e})")),
    };
    let fournisseur = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ServerConfig::builder_with_provider(fournisseur)
        .with_safe_default_protocol_versions()
        .map_err(|e| e.to_string())?
        .with_no_client_auth()
        .with_single_cert(
            vec![CertificateDer::from(certificat.clone())],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(cle)),
        )
        .map_err(|e| format!("le certificat ne sert pas ({e})"))?;
    Ok(Identite { config: Arc::new(config), empreinte: b64url(&connexion::sha256(&certificat)?) })
}

/// Une offre de jumelage ouverte.
struct Offre {
    code: [u8; 32],
    qui: String,
    ouverte: Instant,
    adresses: Vec<IpAddr>,
    /// Le lien du code carré, et le code carré lui-même : ses rangées, « 1 » pour un module noir.
    lien: String,
    qr: Vec<String>,
}

impl Offre {
    fn reste(&self) -> Duration {
        OFFRE_DUREE.saturating_sub(self.ouverte.elapsed())
    }

    fn expiree(&self) -> bool {
        self.reste().is_zero()
    }
}

/// Ce qu'Eve envoie à un téléphone branché sur son flux.
enum Message {
    Etat(Arc<Etat>),
    /// Une phrase de la voix d'Eve : son numéro, le silence avant elle, son WAV.
    Voix {
        numero: u64,
        pause: Duration,
        wav: Vec<u8>,
    },
    /// La voix d'Eve se tait : ce qui se dit et ce qui attend s'oublient.
    Coupe,
}

/// L'état de la fenêtre tel que le voit un téléphone ([`alleger`]) : avec la conversation, et sans elle pour le flux d'un
/// téléphone qui l'a déjà (son numéro, `fil_rev`, n'a pas changé).
struct Etat {
    avec_fil: String,
    sans_fil: String,
    fil_rev: u64,
}

struct Client {
    telephone: String,
    connexion: u64,
    envoi: Sender<Message>,
}

struct Partage {
    local: PathBuf,
    nom: String,
    port: u16,
    ecoute: Ipv4Addr,
    rapport: Sender<Evt>,
    registre: Mutex<Registre>,
    offre: Mutex<Option<Offre>>,
    identite: Mutex<Option<Arc<Identite>>>,
    clients: Mutex<Vec<Client>>,
    /// Le dernier état publié, tel quel : un téléphone qui se branche en reçoit sa version allégée.
    dernier: Mutex<String>,
    refus: Mutex<VecDeque<Instant>>,
    ferme_jusqu_a: Mutex<Option<Instant>>,
    en_marche: AtomicBool,
    connexions: AtomicU64,
    /// Pourquoi le serveur n'écoute pas alors qu'il le devrait (un port pris).
    erreur: Mutex<Option<String>>,
}

fn verrou<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Partage {
    fn identite(&self) -> Result<Arc<Identite>, String> {
        let mut i = verrou(&self.identite);
        if let Some(i) = i.as_ref() {
            return Ok(i.clone());
        }
        let neuve = Arc::new(identite(&self.local)?);
        *i = Some(neuve.clone());
        Ok(neuve)
    }

    /// Le serveur écoute tant qu'un téléphone est jumelé ou qu'une offre est ouverte.
    fn doit_ecouter(&self) -> bool {
        !verrou(&self.registre).telephones.is_empty() || verrou(&self.offre).as_ref().is_some_and(|o| !o.expiree())
    }

    fn ferme(&self) -> bool {
        verrou(&self.ferme_jusqu_a).is_some_and(|t| Instant::now() < t)
    }

    /// Un refus : au journal (avec l'adresse d'où il vient), et compté ; trop de refus ferment le serveur à tous.
    fn refus(&self, de: SocketAddr, pourquoi: &str) {
        let maintenant = Instant::now();
        journal::ecrire(&format!("téléphone : refusé ({pourquoi}), de {}", de.ip()));
        let mut r = verrou(&self.refus);
        r.push_back(maintenant);
        while r.front().is_some_and(|t| maintenant.duration_since(*t) > REFUS_FENETRE) {
            r.pop_front();
        }
        if r.len() >= REFUS_MAX {
            r.clear();
            *verrou(&self.ferme_jusqu_a) = Some(maintenant + FERME_POUR);
            journal::ecrire(&format!(
                "téléphone : {REFUS_MAX} refus en dix minutes ; le serveur se ferme à tous pour dix minutes"
            ));
            let _ = self.rapport.send(Evt::Telephone(Evenement::Ferme));
        }
    }

    /// Le téléphone de la clé que porte cette requête (`Authorization: Bearer …`), s'il est jumelé.
    fn reconnaitre(&self, req: &Requete) -> Option<Telephone> {
        let cle = b64url_decoder(req.entete("authorization")?.strip_prefix("Bearer ")?.trim())?;
        if cle.len() != 32 {
            return None;
        }
        verrou(&self.registre).reconnaitre(&cle)
    }
}

/// Les téléphones de cette Eve : ce que le chef en tient.
#[derive(Clone)]
pub struct Telephones {
    p: Arc<Partage>,
}

impl Telephones {
    /// Les téléphones de ce dossier local ; `nom` : celui de l'Eve (le téléphone le montre) ; `ecoute` : l'adresse du
    /// serveur (`0.0.0.0` : toutes ; `127.0.0.1` : celles d'un essai, avec un émulateur). S'il y a des téléphones
    /// jumelés, le serveur écoute dès maintenant.
    pub fn demarrer(local: PathBuf, nom: String, port: u16, ecoute: Ipv4Addr, rapport: Sender<Evt>) -> Telephones {
        let registre = Registre::ouvrir(&local);
        let jumeles = registre.telephones.len();
        let t = Telephones {
            p: Arc::new(Partage {
                local,
                nom,
                port,
                ecoute,
                rapport,
                registre: Mutex::new(registre),
                offre: Mutex::new(None),
                identite: Mutex::new(None),
                clients: Mutex::new(Vec::new()),
                dernier: Mutex::new(String::new()),
                refus: Mutex::new(VecDeque::new()),
                ferme_jusqu_a: Mutex::new(None),
                en_marche: AtomicBool::new(false),
                connexions: AtomicU64::new(0),
                erreur: Mutex::new(None),
            }),
        };
        if jumeles > 0 {
            t.ecouter();
        }
        t
    }

    /// Ouvre une offre de jumelage (cinq minutes), pour `qui`, la personne devant le poste : le serveur écoute dès
    /// maintenant. Une offre déjà ouverte est remplacée (son code ne vaut plus).
    pub fn jumeler(&self, qui: &str) -> Result<(), String> {
        if verrou(&self.p.registre).illisible {
            return Err(format!("{REGISTRE} ne se lit pas ici : Eve ne peut pas y inscrire un téléphone"));
        }
        let identite = self.p.identite()?;
        let adresses = adresses(self.p.ecoute);
        if adresses.is_empty() {
            return Err("cet ordinateur n'est sur aucun réseau : un téléphone ne pourrait pas le joindre".into());
        }
        let mut code = [0u8; 32];
        connexion::aleatoire(&mut code)?;
        let lien = lien(&self.p.nom, &adresses, self.p.port, &identite.empreinte, &code);
        let qr = code_carre(&lien);
        *verrou(&self.p.offre) =
            Some(Offre { code, qui: qui.to_string(), ouverte: Instant::now(), adresses, lien, qr });
        journal::ecrire("téléphone : une offre de jumelage ouverte, pour cinq minutes");
        self.ecouter();
        Ok(())
    }

    /// Ferme l'offre de jumelage, s'il y en a une.
    pub fn fermer_l_offre(&self) {
        if verrou(&self.p.offre).take().is_some() {
            journal::ecrire("téléphone : l'offre de jumelage est fermée");
        }
    }

    /// Retire ce téléphone : sa clé ne vaut plus, et ses connexions se coupent. Son nom, s'il était jumelé.
    pub fn retirer(&self, id: &str) -> Result<Option<String>, String> {
        let nom = {
            let mut r = verrou(&self.p.registre);
            let Some(i) = r.telephones.iter().position(|t| t.id == id) else { return Ok(None) };
            let t = r.telephones.remove(i);
            if let Err(e) = r.ecrire() {
                r.telephones.insert(i, t);
                return Err(e);
            }
            t.nom
        };
        // Leur envoi s'en va : leur flux finit aussitôt.
        verrou(&self.p.clients).retain(|c| c.telephone != id);
        journal::ecrire(&format!("téléphone : « {nom} » retiré"));
        Ok(Some(nom))
    }

    /// Ce que la fenêtre en montre : les téléphones jumelés (et s'ils sont branchés), l'offre ouverte et son code carré,
    /// ce qui empêche le serveur d'écouter.
    pub fn etat(&self) -> Value {
        let jumeles = verrou(&self.p.registre).telephones.clone();
        let liste: Vec<Value> = jumeles
            .iter()
            .map(|t| {
                json!({ "id": t.id, "nom": t.nom, "qui": t.qui, "jumele": t.jumele, "branche": self.branche(&t.id) })
            })
            .collect();
        let offre = {
            let mut o = verrou(&self.p.offre);
            if o.as_ref().is_some_and(Offre::expiree) {
                *o = None;
                journal::ecrire("téléphone : l'offre de jumelage a expiré");
            }
            o.as_ref().map(|o| {
                let adresses: Vec<String> = o.adresses.iter().map(IpAddr::to_string).collect();
                json!({ "lien": o.lien, "qr": o.qr, "reste": o.reste().as_secs(), "adresses": adresses, "port": self.p.port })
            })
        };
        let erreur = match (self.p.ferme(), verrou(&self.p.erreur).clone()) {
            (true, _) => Some("Trop d'essais refusés : le serveur des téléphones est fermé dix minutes.".to_string()),
            (false, e) => e,
        };
        json!({ "liste": liste, "offre": offre, "erreur": erreur })
    }

    /// L'état de la fenêtre, publié : chaque téléphone branché en reçoit ce qu'il en voit ([`alleger`]).
    pub fn publier(&self, etat: &str) {
        *verrou(&self.p.dernier) = etat.to_string();
        let mut clients = verrou(&self.p.clients);
        if clients.is_empty() {
            return;
        }
        let Some(e) = alleger(etat) else { return };
        let e = Arc::new(e);
        clients.retain(|c| c.envoi.send(Message::Etat(e.clone())).is_ok());
    }

    /// Le renvoi de la voix d'Eve vers ce téléphone, pour le fil de la voix ([`crate::voix::Voix::sortie`]).
    pub fn renvoi(&self, id: &str) -> Renvoi {
        Renvoi { p: self.p.clone(), telephone: id.to_string() }
    }

    /// Ce téléphone est-il branché sur le flux ?
    pub fn branche(&self, id: &str) -> bool {
        verrou(&self.p.clients).iter().any(|c| c.telephone == id)
    }

    /// Le nom de ce téléphone, s'il est jumelé.
    pub fn nom(&self, id: &str) -> Option<String> {
        verrou(&self.p.registre).telephones.iter().find(|t| t.id == id).map(|t| t.nom.clone())
    }

    fn ecouter(&self) {
        if !self.p.en_marche.swap(true, Ordering::SeqCst) {
            let p = self.p.clone();
            thread::spawn(move || ecouter(&p));
        }
    }
}

/// Le renvoi de la voix d'Eve vers un téléphone : le fil de la voix lui donne chaque phrase faite, et la coupure.
#[derive(Clone)]
pub struct Renvoi {
    p: Arc<Partage>,
    telephone: String,
}

impl Renvoi {
    /// Une phrase pour le téléphone, à jouer `pause` après la précédente ; faux s'il n'est pas branché.
    pub fn voix(&self, numero: u64, pause: Duration, wav: &[u8]) -> bool {
        self.envoyer(|| Message::Voix { numero, pause, wav: wav.to_vec() })
    }

    /// La voix se tait sur le téléphone.
    pub fn couper(&self) {
        self.envoyer(|| Message::Coupe);
    }

    fn envoyer(&self, message: impl Fn() -> Message) -> bool {
        let mut envoye = false;
        verrou(&self.p.clients).retain(|c| {
            if c.telephone != self.telephone {
                return true;
            }
            let ok = c.envoi.send(message()).is_ok();
            envoye |= ok;
            ok
        });
        envoye
    }
}

/// L'état de la fenêtre tel que le voit un téléphone : les seuls [`CHAMPS`], la conversation réduite à ses dernières
/// lignes ; et le même sans la conversation.
fn alleger(etat: &str) -> Option<Etat> {
    let v: Value = serde_json::from_str(etat).ok()?;
    let mut o = serde_json::Map::new();
    for c in CHAMPS {
        if let Some(x) = v.get(c) {
            o.insert(c.to_string(), x.clone());
        }
    }
    if let Some(Value::Array(fil)) = o.get_mut("fil") {
        let trop = fil.len().saturating_sub(FIL_MAX);
        fil.drain(..trop);
    }
    let fil_rev = o.get("fil_rev").and_then(Value::as_u64).unwrap_or(0);
    let avec_fil = Value::Object(o.clone()).to_string();
    o.remove("fil");
    Some(Etat { avec_fil, sans_fil: Value::Object(o).to_string(), fil_rev })
}

/// Les adresses de ce poste qu'un téléphone peut joindre : celle de son réseau (le Wi-Fi ou le câble de la maison), et
/// celle de Tailscale s'il est là. Chacune, l'adresse locale d'une prise UDP « connectée » à une adresse de ce réseau —
/// une prise UDP ne fait rien partir en se connectant. Un serveur qui n'écoute qu'une adresse (un essai) : celle-là.
fn adresses(ecoute: Ipv4Addr) -> Vec<IpAddr> {
    if !ecoute.is_unspecified() {
        return vec![IpAddr::V4(ecoute)];
    }
    let mut a = Vec::new();
    // Une adresse de documentation (TEST-NET-1) : la route d'office, celle de la maison ; puis celle de MagicDNS, que
    // Tailscale route chez lui (sans Tailscale, la route d'office encore : la même adresse, écartée).
    for vers in ["192.0.2.1:9", "100.100.100.100:53"] {
        if let Ok(s) = UdpSocket::bind("0.0.0.0:0")
            && s.connect(vers).is_ok()
            && let Ok(l) = s.local_addr()
        {
            let ip = l.ip();
            if !ip.is_loopback() && !ip.is_unspecified() && !a.contains(&ip) {
                a.push(ip);
            }
        }
    }
    a
}

/// Le lien du code carré : `eve://jumeler?v=1&n=<nom de l'Eve>&a=<adresses>&p=<port>&e=<empreinte>&c=<code>`.
fn lien(nom: &str, adresses: &[IpAddr], port: u16, empreinte: &str, code: &[u8]) -> String {
    let a: Vec<String> = adresses.iter().map(IpAddr::to_string).collect();
    format!("eve://jumeler?v=1&n={}&a={}&p={port}&e={empreinte}&c={}", encoder(nom), a.join(","), b64url(code))
}

/// Le codage d'un paramètre d'adresse : les lettres et les chiffres ASCII, et « - _ . ~ », tels quels ; le reste en
/// `%XX`, octet par octet (UTF-8) — l'inverse d'[`ecran::decoder`].
fn encoder(s: &str) -> String {
    let mut sortie = String::with_capacity(s.len());
    for o in s.bytes() {
        if o.is_ascii_alphanumeric() || b"-_.~".contains(&o) {
            sortie.push(char::from(o));
        } else {
            sortie.push_str(&format!("%{o:02X}"));
        }
    }
    sortie
}

/// Le code carré d'un lien (correction M : il se lit encore abîmé de 15 %), en rangées de « 0 » et de « 1 ».
fn code_carre(lien: &str) -> Vec<String> {
    let Ok(q) = qrcode::QrCode::with_error_correction_level(lien.as_bytes(), qrcode::EcLevel::M) else {
        return Vec::new();
    };
    let largeur = q.width();
    q.to_colors()
        .chunks(largeur)
        .map(|rangee| rangee.iter().map(|c| if *c == qrcode::Color::Dark { '1' } else { '0' }).collect())
        .collect()
}

/// L'écoute du serveur, dans son fil : tant qu'elle doit durer ([`Partage::doit_ecouter`]), puis elle se ferme — et se
/// rouvre si une offre est venue pendant qu'elle se fermait.
fn ecouter(p: &Arc<Partage>) {
    loop {
        let ecoute = match TcpListener::bind((p.ecoute, p.port)).and_then(|e| e.set_nonblocking(true).map(|()| e)) {
            Ok(e) => {
                *verrou(&p.erreur) = None;
                e
            }
            Err(e) => {
                let m = format!("Le port {} ne s'ouvre pas pour les téléphones ({e}).", p.port);
                journal::ecrire(&format!("téléphone : {m}"));
                *verrou(&p.erreur) = Some(m);
                p.en_marche.store(false, Ordering::SeqCst);
                return;
            }
        };
        journal::ecrire(&format!("téléphone : le serveur écoute ({}:{})", p.ecoute, p.port));
        while p.doit_ecouter() {
            match ecoute.accept() {
                Ok((flux, de)) => {
                    let p = p.clone();
                    thread::spawn(move || {
                        let _ = servir(flux, de, &p);
                    });
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => thread::sleep(Duration::from_millis(200)),
                Err(_) => thread::sleep(Duration::from_secs(1)),
            }
        }
        drop(ecoute);
        journal::ecrire("téléphone : le serveur n'écoute plus (aucun téléphone jumelé)");
        p.en_marche.store(false, Ordering::SeqCst);
        if !p.doit_ecouter() || p.en_marche.swap(true, Ordering::SeqCst) {
            return;
        }
    }
}

/// Une connexion d'un téléphone : le TLS, puis une requête, puis la fin (le flux, lui, dure).
fn servir(flux: TcpStream, de: SocketAddr, p: &Arc<Partage>) -> io::Result<()> {
    if p.ferme() {
        return Ok(());
    }
    // Sous Windows, une connexion acceptée garde le mode de l'écoute : non bloquant.
    flux.set_nonblocking(false)?;
    flux.set_nodelay(true)?;
    flux.set_read_timeout(Some(Duration::from_secs(20)))?;
    flux.set_write_timeout(Some(Duration::from_secs(20)))?;
    let identite = p.identite().map_err(io::Error::other)?;
    let connexion = rustls::ServerConnection::new(identite.config.clone()).map_err(io::Error::other)?;
    let mut tls = rustls::StreamOwned::new(connexion, flux);
    let Some(req) = ecran::lire_tete(&mut tls)? else { return Ok(()) };
    let resultat = repondre_au_telephone(&mut tls, &req, de, p);
    tls.conn.send_close_notify();
    let _ = tls.flush();
    resultat
}

type Tls = rustls::StreamOwned<rustls::ServerConnection, TcpStream>;

fn repondre_au_telephone(tls: &mut Tls, req: &Requete, de: SocketAddr, p: &Arc<Partage>) -> io::Result<()> {
    if req.methode == "POST" && req.chemin == "/jumeler" {
        return jumeler(tls, req, de, p);
    }
    let Some(t) = p.reconnaitre(req) else {
        p.refus(de, "une clé inconnue");
        // Le corps lu d'abord (un petit seulement) : fermée sur un corps qui arrive encore, la connexion se
        // réinitialise, et le téléphone verrait une panne du réseau au lieu de ce refus.
        let _ = ecran::lire_corps(tls, req, 64 * 1024);
        return ecran::repondre(tls, 401, TEXTE, "Ce téléphone n'est pas jumelé à cette Eve.".as_bytes());
    };
    match (req.methode.as_str(), req.chemin.as_str()) {
        ("GET", "/flux") => flux_du_telephone(tls, &t, p),
        ("POST", "/action") => {
            let corps = ecran::lire_corps(tls, req, 64 * 1024)?;
            let v: Value = serde_json::from_slice(&corps).unwrap_or(Value::Null);
            let action = v["action"].as_str().unwrap_or("");
            if !GESTES.contains(&action) {
                return ecran::repondre(tls, 403, TEXTE, "Ce geste ne se fait pas du téléphone.".as_bytes());
            }
            let geste = Evenement::Geste { telephone: t.id, action: action.to_string(), valeur: v["valeur"].clone() };
            let _ = p.rapport.send(Evt::Telephone(geste));
            ecran::repondre(tls, 204, TEXTE, b"")
        }
        ("POST", "/son") => {
            let n = req.longueur();
            if n == 0 || n > SON_MAX {
                return ecran::repondre(tls, 413, TEXTE, "Un son vide, ou de plus de 45 secondes.".as_bytes());
            }
            tls.sock.set_read_timeout(Some(Duration::from_secs(60)))?;
            let corps = ecran::lire_corps(tls, req, SON_MAX)?;
            match crate::micro::pcm_du_wav(&corps) {
                Ok(audio) => {
                    let son = Evenement::Son { telephone: t.id, audio, fin: Instant::now() };
                    let _ = p.rapport.send(Evt::Telephone(son));
                    ecran::repondre(tls, 204, TEXTE, b"")
                }
                Err(e) => ecran::repondre(tls, 400, TEXTE, e.as_bytes()),
            }
        }
        ("POST", "/piece") => {
            tls.sock.set_read_timeout(Some(Duration::from_secs(60)))?;
            ecran::recevoir_piece(tls, req, &p.rapport)
        }
        ("GET", "/portrait") => portrait(tls, &p.local),
        _ => ecran::repondre(tls, 404, TEXTE, "Introuvable.".as_bytes()),
    }
}

/// Un téléphone se présente avec le code de l'offre ouverte et sa clé : il est jumelé, l'offre se ferme.
fn jumeler(tls: &mut Tls, req: &Requete, de: SocketAddr, p: &Arc<Partage>) -> io::Result<()> {
    let corps = ecran::lire_corps(tls, req, 4096)?;
    let v: Value = serde_json::from_slice(&corps).unwrap_or(Value::Null);
    let code = v["code"].as_str().and_then(b64url_decoder).unwrap_or_default();
    let cle = v["cle"].as_str().and_then(b64url_decoder).unwrap_or_default();
    let qui = {
        let mut offre = verrou(&p.offre);
        let valable = offre.as_ref().is_some_and(|o| !o.expiree() && egaux(&o.code, &code));
        if !valable {
            drop(offre);
            p.refus(de, "un code de jumelage faux ou passé");
            let m = "Ce code ne vaut plus : rouvre « Jumeler un téléphone » sur l'ordinateur.";
            return ecran::repondre(tls, 403, TEXTE, m.as_bytes());
        }
        if cle.len() != 32 {
            return ecran::repondre(tls, 400, TEXTE, "La clé du téléphone doit faire 32 octets.".as_bytes());
        }
        offre.take().map(|o| o.qui).unwrap_or_default()
    };
    let mut numero = [0u8; 8];
    let empreinte = connexion::aleatoire(&mut numero).and_then(|()| connexion::sha256(&cle));
    let Ok(empreinte) = empreinte else {
        return ecran::repondre(tls, 500, TEXTE, "Le générateur du système a échoué.".as_bytes());
    };
    let t = Telephone {
        id: hex(&numero),
        nom: nom_du_telephone(v["nom"].as_str().unwrap_or("")),
        qui,
        jumele: journal::date_et_heure(),
        empreinte: hex(&empreinte),
    };
    {
        let mut r = verrou(&p.registre);
        r.telephones.push(t.clone());
        if let Err(e) = r.ecrire() {
            r.telephones.pop();
            journal::ecrire(&format!("téléphone : « {} » ne s'inscrit pas ({e})", t.nom));
            return ecran::repondre(tls, 500, TEXTE, format!("Eve n'a pas pu l'inscrire : {e}.").as_bytes());
        }
    }
    journal::ecrire(&format!("téléphone : « {} » jumelé, pour {}", t.nom, t.qui));
    let _ = p.rapport.send(Evt::Telephone(Evenement::Jumele { nom: t.nom.clone(), qui: t.qui.clone() }));
    let corps = json!({ "id": t.id, "eve": p.nom, "qui": t.qui }).to_string();
    ecran::repondre(tls, 200, JSON, corps.as_bytes())
}

/// Le nom d'un téléphone, tel qu'il se donne : ses caractères visibles, quarante au plus ; à défaut, « Téléphone ».
fn nom_du_telephone(nom: &str) -> String {
    let propre: String = nom.chars().filter(|c| !c.is_control()).take(40).collect();
    let propre = propre.trim();
    if propre.is_empty() { "Téléphone".to_string() } else { propre.to_string() }
}

/// Le flux d'un téléphone (Server-Sent Events) : l'état allégé de la fenêtre (`etat`), la voix d'Eve (`voix`, un WAV en
/// base64url, et `coupe`), et un battement.
fn flux_du_telephone(tls: &mut Tls, t: &Telephone, p: &Arc<Partage>) -> io::Result<()> {
    tls.sock.set_read_timeout(None)?;
    tls.write_all(
        b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream; charset=utf-8\r\nCache-Control: no-store\r\n\
          X-Content-Type-Options: nosniff\r\nConnection: keep-alive\r\n\r\n",
    )?;
    tls.flush()?;
    let (envoi, recu) = mpsc::channel();
    let connexion = p.connexions.fetch_add(1, Ordering::SeqCst);
    verrou(&p.clients).push(Client { telephone: t.id.clone(), connexion, envoi });
    let _ = p.rapport.send(Evt::Telephone(Evenement::Branche { telephone: t.id.clone() }));
    let resultat = (|| -> io::Result<()> {
        let mut fil_vu = None;
        let initial = alleger(&verrou(&p.dernier));
        if let Some(e) = initial {
            envoyer_etat(tls, &e, &mut fil_vu)?;
        }
        loop {
            let premier = match recu.recv_timeout(BATTEMENT) {
                Ok(m) => m,
                Err(RecvTimeoutError::Timeout) => {
                    tls.write_all(b": battement\n\n")?;
                    tls.flush()?;
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => return Ok(()),
            };
            // Ce qui attendait, dans l'ordre ; de plusieurs états, le dernier seul.
            let lot: Vec<Message> = std::iter::once(premier).chain(recu.try_iter()).collect();
            let dernier_etat = lot.iter().rposition(|m| matches!(m, Message::Etat(_)));
            for (i, m) in lot.into_iter().enumerate() {
                match m {
                    Message::Etat(e) if Some(i) == dernier_etat => envoyer_etat(tls, &e, &mut fil_vu)?,
                    Message::Etat(_) => {}
                    Message::Voix { numero, pause, wav } => {
                        let pause = u64::try_from(pause.as_millis()).unwrap_or(u64::MAX);
                        let donnees = json!({ "numero": numero, "pause": pause, "wav": b64url(&wav) }).to_string();
                        evenement(tls, "voix", &donnees)?;
                    }
                    Message::Coupe => evenement(tls, "coupe", "{}")?,
                }
            }
        }
    })();
    verrou(&p.clients).retain(|c| c.connexion != connexion);
    let _ = p.rapport.send(Evt::Telephone(Evenement::Parti { telephone: t.id.clone() }));
    resultat
}

fn envoyer_etat(flux: &mut impl Write, e: &Etat, fil_vu: &mut Option<u64>) -> io::Result<()> {
    let donnees = if *fil_vu == Some(e.fil_rev) { &e.sans_fil } else { &e.avec_fil };
    *fil_vu = Some(e.fil_rev);
    evenement(flux, "etat", donnees)
}

/// Un événement nommé : ses données sont un JSON d'une seule ligne, donc un seul champ `data`.
fn evenement(flux: &mut impl Write, nom: &str, donnees: &str) -> io::Result<()> {
    flux.write_all(format!("event: {nom}\ndata: ").as_bytes())?;
    flux.write_all(donnees.as_bytes())?;
    flux.write_all(b"\n\n")?;
    flux.flush()
}

/// Le portrait d'Eve, celui que montre la fenêtre (`portrait.png`, ou `.jpg`, `.jpeg`, `.webp`, du dossier local) ; sans
/// portrait, le téléphone garde son orbe.
fn portrait(flux: &mut (impl Read + Write), local: &Path) -> io::Result<()> {
    for (ext, sorte) in [("png", "image/png"), ("jpg", "image/jpeg"), ("jpeg", "image/jpeg"), ("webp", "image/webp")] {
        let chemin = local.join(format!("portrait.{ext}"));
        if fs::metadata(&chemin).is_ok_and(|m| m.is_file() && m.len() <= PORTRAIT_MAX)
            && let Ok(o) = fs::read(&chemin)
        {
            return ecran::repondre(flux, 200, sorte, &o);
        }
    }
    ecran::repondre(flux, 404, TEXTE, "Pas de portrait.".as_bytes())
}

/// Deux suites d'octets égales, comparées en temps constant (une clé, une empreinte).
fn egaux(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn hex(o: &[u8]) -> String {
    o.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod essais {
    use super::*;

    #[test]
    fn le_telephone_voit_l_etat_allege() {
        let fil: Vec<Value> = (0..80).map(|i| json!({ "de": "toi", "texte": format!("ligne {i}") })).collect();
        let etat = json!({
            "etat": "pret", "libelle": "Prête", "fil_rev": 9, "fil": fil, "reglages": { "sourdine": true },
            "epingles": [1], "comptabilite": "C:/Exemple/Comptes", "profils": [], "voix": {}, "telephones": {},
        })
        .to_string();
        let e = alleger(&etat).unwrap();
        let avec: Value = serde_json::from_str(&e.avec_fil).unwrap();
        assert_eq!(avec["fil"].as_array().map(Vec::len), Some(FIL_MAX));
        assert_eq!(avec["fil"][0]["texte"], "ligne 20", "les dernières lignes");
        for absent in ["reglages", "epingles", "comptabilite", "profils", "voix", "telephones"] {
            assert!(avec.get(absent).is_none(), "{absent}");
        }
        let sans: Value = serde_json::from_str(&e.sans_fil).unwrap();
        assert!(sans.get("fil").is_none());
        assert_eq!((sans["libelle"].as_str(), e.fil_rev), (Some("Prête"), 9));
        assert!(alleger("pas du JSON").is_none());
    }

    #[test]
    fn le_lien_et_son_code_carre() {
        let adresses = [IpAddr::V4(Ipv4Addr::new(192, 168, 1, 20)), IpAddr::V4(Ipv4Addr::new(100, 64, 0, 7))];
        let l = lien("Essai d'Été", &adresses, 48822, "EMPREINTE", &[0xFF; 32]);
        let debut = "eve://jumeler?v=1&n=Essai%20d%27%C3%89t%C3%A9&a=192.168.1.20,100.64.0.7&p=48822&e=EMPREINTE&c=";
        assert!(l.starts_with(debut), "{l}");
        assert_eq!(ecran::decoder("Essai%20d%27%C3%89t%C3%A9"), "Essai d'Été");
        assert_eq!(l.rsplit("c=").next().map(str::len), Some(43), "32 octets en base64url");
        let qr = code_carre(&l);
        assert!(qr.len() >= 21, "{}", qr.len());
        assert!(qr.iter().all(|r| r.len() == qr.len() && r.chars().all(|c| c == '0' || c == '1')));
        // Les repères d'un code carré : ses coins noirs.
        assert!(qr[0].starts_with("1111111") && qr[qr.len() - 1].starts_with("1111111"));
    }

    #[test]
    fn le_nom_d_un_telephone() {
        assert_eq!(nom_du_telephone("  Pixel 8  "), "Pixel 8");
        assert_eq!(nom_du_telephone("\u{0}\r\n"), "Téléphone");
        assert_eq!(nom_du_telephone(&"x".repeat(90)).chars().count(), 40);
    }

    #[test]
    fn des_octets_egaux_en_temps_constant() {
        assert!(egaux(b"abc", b"abc"));
        assert!(!egaux(b"abc", b"abd") && !egaux(b"abc", b"ab"));
        assert_eq!(hex(&[0x00, 0xAB, 0x7F]), "00ab7f");
    }

    /// Ce que fait l'appli : n'accepter que le certificat de cette empreinte, et rien de ce que dit sa chaîne.
    #[cfg(windows)]
    mod client {
        use std::sync::Arc;
        use std::time::Duration;

        use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
        use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
        use rustls::{DigitallySignedStruct, SignatureScheme};

        #[derive(Debug)]
        struct Epingle(String);

        fn algorithmes() -> rustls::crypto::WebPkiSupportedAlgorithms {
            rustls::crypto::ring::default_provider().signature_verification_algorithms
        }

        impl ServerCertVerifier for Epingle {
            fn verify_server_cert(
                &self,
                fin: &CertificateDer<'_>,
                _: &[CertificateDer<'_>],
                _: &ServerName<'_>,
                _: &[u8],
                _: UnixTime,
            ) -> Result<ServerCertVerified, rustls::Error> {
                let e = crate::connexion::b64url(&crate::connexion::sha256(fin.as_ref()).unwrap_or_default());
                if e == self.0 {
                    Ok(ServerCertVerified::assertion())
                } else {
                    Err(rustls::Error::General("une autre empreinte".into()))
                }
            }

            fn verify_tls12_signature(
                &self,
                m: &[u8],
                c: &CertificateDer<'_>,
                d: &DigitallySignedStruct,
            ) -> Result<HandshakeSignatureValid, rustls::Error> {
                rustls::crypto::verify_tls12_signature(m, c, d, &algorithmes())
            }

            fn verify_tls13_signature(
                &self,
                m: &[u8],
                c: &CertificateDer<'_>,
                d: &DigitallySignedStruct,
            ) -> Result<HandshakeSignatureValid, rustls::Error> {
                rustls::crypto::verify_tls13_signature(m, c, d, &algorithmes())
            }

            fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
                algorithmes().supported_schemes()
            }
        }

        pub type Flux = rustls::StreamOwned<rustls::ClientConnection, std::net::TcpStream>;

        pub fn connecter(port: u16, empreinte: &str) -> Flux {
            let config =
                rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                    .with_safe_default_protocol_versions()
                    .unwrap()
                    .dangerous()
                    .with_custom_certificate_verifier(Arc::new(Epingle(empreinte.to_string())))
                    .with_no_client_auth();
            let c = rustls::ClientConnection::new(Arc::new(config), ServerName::try_from("eve").unwrap()).unwrap();
            let s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
            s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
            rustls::StreamOwned::new(c, s)
        }
    }

    /// Une requête de l'appli : son code et sa réponse entière.
    #[cfg(windows)]
    fn demande(
        port: u16,
        empreinte: &str,
        methode: &str,
        chemin: &str,
        cle: Option<&str>,
        corps: &[u8],
    ) -> (u16, String) {
        let mut f = client::connecter(port, empreinte);
        let cle = cle.map(|c| format!("Authorization: Bearer {c}\r\n")).unwrap_or_default();
        let tete = format!(
            "{methode} {chemin} HTTP/1.1\r\nHost: eve\r\n{cle}Content-Length: {}\r\nConnection: close\r\n\r\n",
            corps.len()
        );
        f.write_all(tete.as_bytes()).unwrap();
        f.write_all(corps).unwrap();
        f.flush().unwrap();
        let mut r = Vec::new();
        let _ = f.read_to_end(&mut r);
        let r = String::from_utf8_lossy(&r).into_owned();
        (r.split(' ').nth(1).and_then(|c| c.parse().ok()).unwrap_or(0), r)
    }

    /// Le prochain événement du flux : son nom et ses données (les battements passent) ; `None` : le flux s'est fermé.
    #[cfg(windows)]
    fn evenement_lu(l: &mut impl std::io::BufRead) -> Option<(String, String)> {
        let (mut nom, mut donnees) = (String::new(), String::new());
        loop {
            let mut ligne = String::new();
            if l.read_line(&mut ligne).ok()? == 0 {
                return None;
            }
            let ligne = ligne.trim_end_matches(['\r', '\n']);
            if ligne.is_empty() {
                if !donnees.is_empty() {
                    return Some((nom, donnees));
                }
            } else if let Some(n) = ligne.strip_prefix("event: ") {
                nom = n.to_string();
            } else if let Some(d) = ligne.strip_prefix("data: ") {
                donnees = d.to_string();
            }
        }
    }

    #[cfg(windows)]
    fn wav_16k(echantillons: &[i16]) -> Vec<u8> {
        let n = (echantillons.len() * 2) as u32;
        let mut o = Vec::new();
        o.extend_from_slice(b"RIFF");
        o.extend_from_slice(&(36 + n).to_le_bytes());
        o.extend_from_slice(b"WAVEfmt ");
        o.extend_from_slice(&16u32.to_le_bytes());
        o.extend_from_slice(&1u16.to_le_bytes());
        o.extend_from_slice(&1u16.to_le_bytes());
        o.extend_from_slice(&16_000u32.to_le_bytes());
        o.extend_from_slice(&32_000u32.to_le_bytes());
        o.extend_from_slice(&2u16.to_le_bytes());
        o.extend_from_slice(&16u16.to_le_bytes());
        o.extend_from_slice(b"data");
        o.extend_from_slice(&n.to_le_bytes());
        for e in echantillons {
            o.extend_from_slice(&e.to_le_bytes());
        }
        o
    }

    #[cfg(windows)]
    fn evenement_du_chef(recu: &mpsc::Receiver<Evt>) -> Evenement {
        loop {
            match recu.recv_timeout(Duration::from_secs(10)) {
                Ok(Evt::Telephone(e)) => return e,
                Ok(_) => {}
                Err(e) => panic!("rien du serveur des téléphones : {e}"),
            }
        }
    }

    /// Le parcours d'un téléphone, par le vrai serveur et un client TLS qui fait comme l'appli : sans téléphone, Eve
    /// n'écoute pas ; un code faux est refusé ; le bon jumelle, une seule fois ; le registre est chiffré ; une requête
    /// sans clé, un geste hors de la conversation sont refusés ; un geste, une voix vont au chef ; le flux porte l'état
    /// allégé (la conversation quand elle change), la voix d'Eve et sa coupure ; retiré, le téléphone perd son flux et
    /// sa clé.
    #[cfg(windows)]
    #[test]
    fn un_telephone_se_jumelle_parle_et_se_retire() {
        use std::io::BufReader;

        let local = std::env::temp_dir().join(format!("eve-essai-telephone-{}", std::process::id()));
        let _ = fs::remove_dir_all(&local);
        fs::create_dir_all(&local).unwrap();
        let port = TcpListener::bind(("127.0.0.1", 0)).unwrap().local_addr().unwrap().port();
        let (envoi, recu) = mpsc::channel();
        let t = Telephones::demarrer(local.clone(), "Essai".into(), port, Ipv4Addr::LOCALHOST, envoi);
        assert!(!t.p.en_marche.load(Ordering::SeqCst), "sans téléphone, Eve n'écoute pas");

        t.jumeler("Sam").unwrap();
        let lien = t.etat()["offre"]["lien"].as_str().unwrap().to_string();
        let parametre = |nom: &str| {
            lien.split(['?', '&']).find_map(|p| p.strip_prefix(&format!("{nom}="))).unwrap_or_default().to_string()
        };
        let attendu = ("Essai".to_string(), "127.0.0.1".to_string(), port.to_string());
        assert_eq!((parametre("n"), parametre("a"), parametre("p")), attendu);
        let (empreinte, code) = (parametre("e"), parametre("c"));
        let debut = Instant::now();
        while TcpStream::connect(("127.0.0.1", port)).is_err() {
            assert!(debut.elapsed() < Duration::from_secs(10), "le serveur n'écoute pas");
            thread::sleep(Duration::from_millis(50));
        }

        let cle = b64url(&[7u8; 32]);
        let presentation = |code: &str| json!({ "code": code, "cle": cle, "nom": "Pixel d'essai" }).to_string();
        let faux = b64url(&[0u8; 32]);
        assert_eq!(demande(port, &empreinte, "POST", "/jumeler", None, presentation(&faux).as_bytes()).0, 403);
        let (c, r) = demande(port, &empreinte, "POST", "/jumeler", None, presentation(&code).as_bytes());
        assert_eq!(c, 200, "{r}");
        let v: Value = serde_json::from_str(r.rsplit("\r\n\r\n").next().unwrap()).unwrap();
        assert_eq!((v["eve"].as_str(), v["qui"].as_str()), (Some("Essai"), Some("Sam")));
        let id = v["id"].as_str().unwrap().to_string();
        match evenement_du_chef(&recu) {
            Evenement::Jumele { nom, qui } => assert_eq!((nom.as_str(), qui.as_str()), ("Pixel d'essai", "Sam")),
            _ => panic!("le jumelage n'est pas allé au chef"),
        }
        assert_eq!(demande(port, &empreinte, "POST", "/jumeler", None, presentation(&code).as_bytes()).0, 403);
        assert!(t.etat()["offre"].is_null(), "l'offre se ferme au premier jumelage");
        let brut = fs::read(local.join(REGISTRE)).unwrap();
        assert!(!brut.windows(5).any(|w| w == b"Pixel"), "le registre est chiffré");
        assert_eq!(Registre::ouvrir(&local).telephones.len(), 1);

        let texte = br#"{"action":"texte","valeur":"bonjour"}"#;
        assert_eq!(demande(port, &empreinte, "POST", "/action", None, texte).0, 401);
        assert_eq!(demande(port, &empreinte, "POST", "/action", Some(&cle), br#"{"action":"quitter"}"#).0, 403);
        assert_eq!(demande(port, &empreinte, "POST", "/action", Some(&cle), texte).0, 204);
        match evenement_du_chef(&recu) {
            Evenement::Geste { telephone, action, valeur } => {
                let vu = (telephone.as_str(), action.as_str(), valeur.as_str());
                assert_eq!(vu, (id.as_str(), "texte", Some("bonjour")));
            }
            _ => panic!("le geste n'est pas allé au chef"),
        }
        assert_eq!(demande(port, &empreinte, "POST", "/son", Some(&cle), &wav_16k(&[1000; 16_000])).0, 204);
        match evenement_du_chef(&recu) {
            Evenement::Son { telephone, audio, .. } => assert_eq!((telephone, audio.len()), (id.clone(), 16_000)),
            _ => panic!("la voix n'est pas allée au chef"),
        }

        let etat = |libelle: &str| {
            json!({ "etat": "pret", "libelle": libelle, "fil_rev": 1, "fil": [{ "de": "eve", "texte": "Bonjour" }],
                    "comptabilite": "C:/Exemple/Comptes" })
            .to_string()
        };
        t.publier(&etat("Prête"));
        let mut f = client::connecter(port, &empreinte);
        let requete = format!("GET /flux HTTP/1.1\r\nHost: eve\r\nAuthorization: Bearer {cle}\r\n\r\n");
        f.write_all(requete.as_bytes()).unwrap();
        f.flush().unwrap();
        let mut l = BufReader::new(f);
        let (nom, d) = evenement_lu(&mut l).unwrap();
        let v: Value = serde_json::from_str(&d).unwrap();
        assert_eq!((nom.as_str(), v["fil"][0]["texte"].as_str()), ("etat", Some("Bonjour")));
        assert!(v.get("comptabilite").is_none());
        match evenement_du_chef(&recu) {
            Evenement::Branche { telephone } => assert_eq!(telephone, id),
            _ => panic!("le flux ne s'est pas dit au chef"),
        }
        assert!(t.branche(&id) && t.etat()["liste"][0]["branche"] == true);
        t.publier(&etat("Je réponds"));
        let (nom, d) = evenement_lu(&mut l).unwrap();
        let v: Value = serde_json::from_str(&d).unwrap();
        assert_eq!((nom.as_str(), v["libelle"].as_str()), ("etat", Some("Je réponds")));
        assert!(v.get("fil").is_none(), "la conversation n'a pas changé : elle ne repart pas");
        assert!(t.renvoi(&id).voix(3, Duration::from_millis(300), b"RIFF-son"));
        let (nom, d) = evenement_lu(&mut l).unwrap();
        let v: Value = serde_json::from_str(&d).unwrap();
        assert_eq!((nom.as_str(), v["numero"].as_u64(), v["pause"].as_u64()), ("voix", Some(3), Some(300)));
        assert_eq!(v["wav"].as_str().and_then(b64url_decoder).as_deref(), Some(&b"RIFF-son"[..]));
        t.renvoi(&id).couper();
        assert_eq!(evenement_lu(&mut l).map(|(n, _)| n).as_deref(), Some("coupe"));
        assert!(!t.renvoi("un-autre").voix(4, Duration::ZERO, b"RIFF"), "un téléphone qui n'est pas branché");

        assert_eq!(t.retirer(&id).unwrap().as_deref(), Some("Pixel d'essai"));
        assert!(evenement_lu(&mut l).is_none(), "retiré, son flux se ferme");
        assert!(Registre::ouvrir(&local).telephones.is_empty());
        // Sans téléphone ni offre, Eve n'écoute plus le réseau.
        let debut = Instant::now();
        while TcpStream::connect(("127.0.0.1", port)).is_ok() {
            assert!(debut.elapsed() < Duration::from_secs(10), "le serveur écoute encore");
            thread::sleep(Duration::from_millis(100));
        }
        // Une offre le rouvre ; la clé du téléphone retiré n'y vaut plus rien.
        t.jumeler("Sam").unwrap();
        let debut = Instant::now();
        while TcpStream::connect(("127.0.0.1", port)).is_err() {
            assert!(debut.elapsed() < Duration::from_secs(10), "le serveur ne se rouvre pas");
            thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(demande(port, &empreinte, "POST", "/action", Some(&cle), texte).0, 401);
        t.fermer_l_offre();
        let _ = fs::remove_dir_all(&local);
    }
}
