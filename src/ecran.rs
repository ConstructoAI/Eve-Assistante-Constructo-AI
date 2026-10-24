//! La vitrine : un petit serveur HTTP sur 127.0.0.1, sans dépendance, qui sert la page (`page/eve.html`,
//! embarquée dans le programme), pousse l'état par Server-Sent Events (`/flux`), reçoit les gestes (`POST /action`),
//! les pièces jointes (`POST /piece`) et les numérisations de la caméra (`POST /scan`), et rend l'historique des
//! conversations (`GET /historique`, `GET /historique/conversation`).
//!
//! Un jeton tiré au démarrage protège tout : sans lui, ni la page, ni le flux, ni un geste, ni une pièce. Une page web
//! ouverte dans un navigateur du poste ne peut donc ni lire Eve ni lui faire confirmer une écriture (l'en-tête
//! `X-Eve` exige une requête que le navigateur refuse d'envoyer d'une autre origine sans permission), et l'en-tête
//! `Host` doit nommer 127.0.0.1 (pas de piège par un nom de domaine qui pointerait ici). Une seule adresse se passe du
//! jeton : le retour de la page de connexion de Constructo AI (`/connexion/retour`), que le navigateur ouvre de
//! lui-même ; c'est le `state` de la connexion en cours qui le reconnaît, et un retour qui ne le porte pas est écarté.

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde_json::Value;

use crate::chef::Evt;
use crate::historique;
use crate::reglages::{dossier_local, ecrire_d_un_geste};

const PAGE: &str = include_str!("../page/eve.html");
/// Une pièce jointe : 40 Mo au plus (la même limite que les pièces de la base).
const PIECE_MAX: u64 = 40 * 1024 * 1024;
/// Le battement du flux : un client parti se voit au plus tard au battement suivant.
const BATTEMENT: Duration = Duration::from_secs(5);
/// La politique de la page : ses scripts et ses styles en ligne, les polices d'EG (Google Fonts), rien d'autre.
const POLITIQUE: &str = "default-src 'self'; script-src 'unsafe-inline'; style-src 'self' 'unsafe-inline' \
                         https://fonts.googleapis.com; font-src https://fonts.gstatic.com; connect-src 'self'; \
                         img-src 'self' data:; frame-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

#[derive(Clone)]
pub struct Ecran {
    partage: Arc<Partage>,
    pub port: u16,
    pub jeton: String,
    /// Le jeton des aperçus (`GET /apercu`), distinct : une page affichée en aperçu voit son adresse, pas celui-ci.
    jeton_apercu: String,
}

struct Partage {
    etat: Mutex<String>,
    abonnes: Mutex<Vec<Sender<String>>>,
    actifs: AtomicUsize,
}

/// L'abonnement de la fenêtre native ([`Ecran::abonner`]) : l'état courant, les suivants, et sa présence.
pub struct Abonnement {
    pub initial: String,
    pub recu: mpsc::Receiver<String>,
    pub presence: Presence,
}

/// La fenêtre native compte parmi les fenêtres ouvertes dès qu'elle a dessiné une image ([`Presence::arriver`]), et
/// tant qu'elle vit ; perdue, elle n'y compte plus. Pas avant sa première image : une fenêtre qui ne s'ouvre pas
/// (OpenGL refusé, un blocage) ne passe pas pour ouverte — la marque de santé d'une nouvelle version l'attend
/// (`mise_a_jour.rs`).
pub struct Presence {
    partage: Arc<Partage>,
    arrivee: bool,
}

impl Presence {
    /// La fenêtre a dessiné une image : elle compte désormais (une fois).
    pub fn arriver(&mut self) {
        if !std::mem::replace(&mut self.arrivee, true) {
            self.partage.actifs.fetch_add(1, Ordering::SeqCst);
        }
    }
}

impl Drop for Presence {
    fn drop(&mut self) {
        if self.arrivee {
            self.partage.actifs.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

/// Ouvre la vitrine sur 127.0.0.1:`port`. Un port déjà pris (`AddrInUse`) veut dire qu'une Eve tourne déjà.
pub fn demarrer(port: u16, jeton: String, rapport: Sender<Evt>) -> io::Result<Ecran> {
    let ecoute = TcpListener::bind(("127.0.0.1", port))?;
    let ecran = Ecran {
        partage: Arc::new(Partage {
            etat: Mutex::new("{}".into()),
            abonnes: Mutex::new(Vec::new()),
            actifs: AtomicUsize::new(0),
        }),
        port,
        jeton,
        jeton_apercu: jeton_aleatoire(),
    };
    let e = ecran.clone();
    thread::spawn(move || {
        for flux in ecoute.incoming().flatten() {
            let (e, r) = (e.clone(), rapport.clone());
            thread::spawn(move || {
                let _ = servir(flux, &e, &r);
            });
        }
    });
    Ok(ecran)
}

impl Ecran {
    pub fn adresse(&self) -> String {
        format!("http://127.0.0.1:{}/?t={}", self.port, self.jeton)
    }

    /// Pousse l'état (un JSON d'une ligne) à toutes les fenêtres ouvertes.
    pub fn publier(&self, etat: &str) {
        if let Ok(mut e) = self.partage.etat.lock() {
            *e = etat.to_string();
        }
        if let Ok(mut a) = self.partage.abonnes.lock() {
            a.retain(|s| s.send(etat.to_string()).is_ok());
        }
    }

    /// Un abonné hors HTTP : la fenêtre native. Il reçoit l'état courant, puis chaque changement, et compte parmi les
    /// fenêtres ouvertes, dès sa première image, tant que sa présence vit — comme une page branchée sur `/flux`.
    pub fn abonner(&self) -> Abonnement {
        let (tx, rx) = mpsc::channel::<String>();
        let initial = self.partage.etat.lock().map(|e| e.clone()).unwrap_or_default();
        if let Ok(mut a) = self.partage.abonnes.lock() {
            a.push(tx);
        }
        Abonnement { initial, recu: rx, presence: Presence { partage: self.partage.clone(), arrivee: false } }
    }

    /// Le nombre de fenêtres branchées sur le flux.
    pub fn clients(&self) -> usize {
        self.partage.actifs.load(Ordering::SeqCst)
    }

    /// Note le port et le jeton de cette séance, pour qu'un second lancement rouvre la même fenêtre (d'un seul geste : un
    /// second lancement ne lit jamais une session à moitié écrite) ; et pour `eve --mettre-a-jour`, son processus, et
    /// qu'elle sait redémarrer dans une autre version (`mise_a_jour` : la version de ce geste, `mise_a_jour.rs`).
    pub fn noter_session(&self) {
        let s =
            serde_json::json!({ "port": self.port, "jeton": self.jeton, "pid": std::process::id(), "mise_a_jour": 1 });
        let _ = ecrire_d_un_geste(&dossier_local().join("session.json"), s.to_string().as_bytes());
    }
}

/// Ce que `session.json` dit de l'Eve qui tourne (ou a tourné) avec ce dossier local : où la joindre, son processus, et
/// si elle sait redémarrer dans une autre version (`mise_a_jour` au moins 1 ; 0 : une version d'avant).
pub struct Session {
    pub port: u16,
    pub jeton: String,
    pub pid: u32,
    pub mise_a_jour: u64,
}

/// La séance notée par la dernière Eve lancée avec ce dossier local ([`Ecran::noter_session`]).
pub fn session_notee() -> Option<Session> {
    let s: Value = serde_json::from_str(&fs::read_to_string(dossier_local().join("session.json")).ok()?).ok()?;
    Some(Session {
        port: u16::try_from(s.get("port")?.as_u64()?).ok()?,
        jeton: s.get("jeton")?.as_str()?.to_string(),
        pid: s.get("pid").and_then(Value::as_u64).and_then(|p| u32::try_from(p).ok()).unwrap_or(0),
        mise_a_jour: s.get("mise_a_jour").and_then(Value::as_u64).unwrap_or(0),
    })
}

/// L'adresse de l'Eve qui tourne déjà (`session.json`).
pub fn adresse_existante() -> Option<String> {
    session_notee().map(|s| format!("http://127.0.0.1:{}/?t={}", s.port, s.jeton))
}

/// Un geste pour l'Eve qui tourne, comme sa page l'envoie (`POST /action`, son jeton dans `X-Eve`) : celui de
/// `eve --mettre-a-jour`, qui lui demande de redémarrer dans une autre version. Personne n'écoute sur ce port :
/// `ConnectionRefused` (Windows met deux secondes à le dire, d'où l'attente de cinq).
pub fn geste_a_l_eve(port: u16, jeton: &str, action: &str, valeur: &Value) -> io::Result<()> {
    let corps = serde_json::json!({ "action": action, "valeur": valeur }).to_string();
    let adresse = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let mut flux = TcpStream::connect_timeout(&adresse, Duration::from_secs(5))?;
    flux.set_read_timeout(Some(Duration::from_secs(5)))?;
    flux.set_write_timeout(Some(Duration::from_secs(5)))?;
    write!(
        flux,
        "POST /action HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Eve: {jeton}\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{corps}",
        corps.len()
    )?;
    flux.flush()?;
    let mut reponse = Vec::new();
    flux.read_to_end(&mut reponse)?;
    let reponse = String::from_utf8_lossy(&reponse);
    match reponse.split(' ').nth(1) {
        Some("204") => Ok(()),
        code => Err(io::Error::other(format!("la vitrine répond {}", code.unwrap_or("rien")))),
    }
}

/// Un jeton de 128 bits, propre à la séance (les clés de hachage aléatoires de la bibliothèque standard).
pub fn jeton_aleatoire() -> String {
    use std::hash::{BuildHasher, Hasher};
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    (0..2u64)
        .map(|i| {
            let mut h = std::collections::hash_map::RandomState::new().build_hasher();
            h.write_u64(i);
            h.write_u128(nanos);
            h.write_u32(std::process::id());
            format!("{:016x}", h.finish())
        })
        .collect()
}

/// Ouvre la fenêtre : Edge en mode application (sans barre d'adresse), avec son propre profil (la fenêtre fermée,
/// l'instance d'Edge se ferme, et Eve le voit) ; à défaut, le navigateur par défaut.
pub fn ouvrir_fenetre(url: &str) {
    let edge = [
        r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
        r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
    ]
    .into_iter()
    .map(PathBuf::from)
    .find(|p| p.is_file());
    let resultat = match edge {
        Some(e) => Command::new(e)
            .arg(format!("--app={url}"))
            .arg(format!("--user-data-dir={}", dossier_local().join("edge").display()))
            .args([
                "--window-size=480,900", "--no-first-run", "--no-default-browser-check", "--disable-features=Translate",
            ])
            // Le déclic de la caméra se fait entendre même quand c'est la voix qui l'a demandé (sans clic dans la page).
            .arg("--autoplay-policy=no-user-gesture-required")
            .spawn(),
        // L'adresse ne porte que des chiffres hexadécimaux après « ?t= » : rien que cmd.exe interprète.
        None => Command::new("cmd").args(["/c", "start", "", url]).spawn(),
    };
    if let Err(e) = resultat {
        crate::journal::ecrire(&format!("La fenêtre ne s'ouvre pas ({e}) : {url}"));
    }
}

struct Requete {
    methode: String,
    chemin: String,
    requete: String,
    entetes: Vec<(String, String)>,
    /// Les octets du corps déjà lus avec l'en-tête.
    reste: Vec<u8>,
}

impl Requete {
    fn entete(&self, nom: &str) -> Option<&str> {
        self.entetes.iter().find(|(n, _)| n == nom).map(|(_, v)| v.as_str())
    }

    fn parametre(&self, nom: &str) -> Option<String> {
        self.requete.split('&').filter_map(|p| p.split_once('=')).find(|(n, _)| *n == nom).map(|(_, v)| decoder(v))
    }

    fn longueur(&self) -> u64 {
        self.entete("content-length").and_then(|v| v.parse().ok()).unwrap_or(0)
    }
}

fn lire_tete(flux: &mut TcpStream) -> io::Result<Option<Requete>> {
    let mut tampon = Vec::with_capacity(2048);
    let mut morceau = [0u8; 2048];
    let fin = loop {
        let n = flux.read(&mut morceau)?;
        if n == 0 {
            return Ok(None);
        }
        tampon.extend_from_slice(&morceau[..n]);
        if let Some(p) = tampon.windows(4).position(|w| w == b"\r\n\r\n") {
            break p;
        }
        if tampon.len() > 32 * 1024 {
            return Ok(None);
        }
    };
    let tete = String::from_utf8_lossy(&tampon[..fin]).into_owned();
    let reste = tampon[fin + 4..].to_vec();
    let mut lignes = tete.split("\r\n");
    let mut premiere = lignes.next().unwrap_or("").split(' ');
    let methode = premiere.next().unwrap_or("").to_string();
    let cible = premiere.next().unwrap_or("/");
    let (chemin, requete) = cible.split_once('?').unwrap_or((cible, ""));
    let entetes = lignes
        .filter_map(|l| l.split_once(':'))
        .map(|(n, v)| (n.trim().to_ascii_lowercase(), v.trim().to_string()))
        .collect();
    Ok(Some(Requete { methode, chemin: chemin.to_string(), requete: requete.to_string(), entetes, reste }))
}

/// La page servie : ses jetons, et le nom affiché (`reglages::nom_affiche`) dans son titre — c'est par lui que `fenetre`
/// la retrouve — et sous « Eve ». La page porte celui de l'Adjointe ; un autre nom le remplace.
fn page_servie(jeton: &str, apercu: &str, nom: &str) -> String {
    let page = PAGE.replace("__JETON__", jeton).replace("__APERCU__", apercu);
    if nom == "Adjointe" {
        return page;
    }
    let nom = nom.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    let titre = format!("<title>{}</title>", crate::fenetre::titre_pour(&nom));
    let page = page.replacen("<title>Eve — Adjointe</title>", &titre, 1);
    page.replacen(">Adjointe · Entrepreneur Général</span>", &format!(">{nom}</span>"), 1)
}

fn servir(mut flux: TcpStream, ecran: &Ecran, rapport: &Sender<Evt>) -> io::Result<()> {
    flux.set_read_timeout(Some(Duration::from_secs(15)))?;
    let Some(req) = lire_tete(&mut flux)? else { return Ok(()) };
    let hotes = [format!("127.0.0.1:{}", ecran.port), format!("localhost:{}", ecran.port)];
    if !req.entete("host").is_some_and(|h| hotes.iter().any(|x| x == h)) {
        return repondre(&mut flux, 403, "text/plain; charset=utf-8", "Interdit.".as_bytes());
    }
    let par_parametre = req.parametre("t").as_deref() == Some(ecran.jeton.as_str());
    let par_entete = req.entete("x-eve") == Some(ecran.jeton.as_str());
    match (req.methode.as_str(), req.chemin.as_str()) {
        ("GET", "/") if par_parametre => {
            let page = page_servie(&ecran.jeton, &ecran.jeton_apercu, crate::reglages::nom_affiche());
            let tete = entete_http(200, "text/html; charset=utf-8", page.len(), Some(POLITIQUE));
            flux.write_all(tete.as_bytes())?;
            flux.write_all(page.as_bytes())?;
            flux.flush()
        }
        ("GET", "/") => {
            repondre(&mut flux, 403, "text/plain; charset=utf-8", "Ouvre Eve par son lanceur (Eve.bat).".as_bytes())
        }
        ("GET", "/flux") if par_parametre => flux_d_etat(flux, ecran),
        ("POST", "/action") if par_entete => {
            let corps = lire_corps(&mut flux, &req, 64 * 1024)?;
            if let Ok(v) = serde_json::from_slice::<Value>(&corps) {
                let action = v.get("action").and_then(Value::as_str).unwrap_or("").to_string();
                let valeur = v.get("valeur").cloned().unwrap_or(Value::Null);
                let _ = rapport.send(Evt::Ui { action, valeur });
            }
            repondre(&mut flux, 204, "text/plain", b"")
        }
        ("POST", "/piece") if par_entete => recevoir_piece(flux, &req, rapport),
        ("POST", "/scan") if par_entete => recevoir_scan(flux, &req, rapport),
        ("GET", "/historique") if par_entete => {
            let qui = req.parametre("qui").unwrap_or_default();
            let liste = historique::conversations(&qui, &req.parametre("q").unwrap_or_default(), 300);
            let corps = serde_json::json!({ "conversations": liste }).to_string();
            repondre(&mut flux, 200, "application/json; charset=utf-8", corps.as_bytes())
        }
        ("GET", "/historique/conversation") if par_entete => {
            let qui = req.parametre("qui").unwrap_or_default();
            let echanges = historique::conversation(&qui, &req.parametre("id").unwrap_or_default());
            let corps = serde_json::json!({ "echanges": echanges }).to_string();
            repondre(&mut flux, 200, "application/json; charset=utf-8", corps.as_bytes())
        }
        // La page se ferme (navigator.sendBeacon ne pose pas d'en-tête : le jeton vient en paramètre).
        ("POST", "/fermeture") if par_parametre => {
            let _ = rapport.send(Evt::Ui { action: "fenetre_fermee".into(), valeur: Value::Null });
            repondre(&mut flux, 204, "text/plain", b"")
        }
        ("GET", "/apercu") if req.parametre("a").as_deref() == Some(ecran.jeton_apercu.as_str()) => {
            servir_apercu(flux, &req.parametre("chemin").unwrap_or_default())
        }
        // Le retour de la page de connexion de Constructo AI : le navigateur du poste l'ouvre sans le jeton de la
        // vitrine — c'est le `state` de la connexion en cours qui le reconnaît (le chef le vérifie). La page rendue ne
        // répète rien de ce qu'elle a reçu.
        ("GET", crate::connexion::CHEMIN_RETOUR) => {
            let retour = crate::connexion::Retour {
                code: req.parametre("code").unwrap_or_default(),
                etat_oauth: req.parametre("state").unwrap_or_default(),
                emetteur: req.parametre("iss").unwrap_or_default(),
                erreur: req.parametre("error").unwrap_or_default(),
                description: req.parametre("error_description").unwrap_or_default(),
            };
            let refuse = !retour.erreur.is_empty();
            let _ = rapport.send(Evt::Retour(retour));
            let page = page_de_retour(refuse);
            flux.write_all(
                entete_http(200, "text/html; charset=utf-8", page.len(), Some(POLITIQUE_RETOUR)).as_bytes(),
            )?;
            flux.write_all(page.as_bytes())?;
            flux.flush()
        }
        ("GET", "/favicon.ico") => repondre(&mut flux, 204, "image/x-icon", b""),
        _ => repondre(&mut flux, 404, "text/plain; charset=utf-8", "Introuvable.".as_bytes()),
    }
}

/// La page du retour de connexion : du texte et ses styles, rien d'autre.
const POLITIQUE_RETOUR: &str =
    "default-src 'none'; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

/// La page que le navigateur montre au retour de la connexion à Constructo AI, aux couleurs de la fenêtre d'Eve (la
/// barre marine de Constructo AI). Elle ne sait pas encore si la connexion aboutira (le code s'échange ensuite) : elle
/// renvoie à la fenêtre d'Eve, qui le dit.
fn page_de_retour(refuse: bool) -> String {
    let (titre, texte) = if refuse {
        (
            "La connexion n'a pas abouti",
            "Revenez à la fenêtre d'Eve : elle dit pourquoi. Vous pouvez fermer cet onglet.",
        )
    } else {
        (
            "Autorisation reçue",
            "Eve termine sa connexion à Constructo AI : revenez à sa fenêtre. Vous pouvez fermer cet onglet.",
        )
    };
    format!(
        "<!doctype html><html lang=\"fr-CA\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" \
         content=\"width=device-width, initial-scale=1\"><title>Eve — Constructo AI</title><style>\
         :root{{color-scheme:light dark}}\
         body{{margin:0;min-height:100vh;display:grid;place-items:center;font:15px/1.5 'Segoe UI',system-ui,sans-serif;\
         background:linear-gradient(160deg,#dbeafe,#bfdbfe);color:#1f2937}}\
         main{{max-width:420px;margin:16px;background:#fff;border-radius:12px;box-shadow:0 12px 32px -16px rgba(0,32,80,.45);\
         overflow:hidden}}header{{background:linear-gradient(180deg,#002B6B,#002050);color:#fff;padding:14px 20px;\
         font-weight:600;letter-spacing:.04em}}p{{margin:0;padding:18px 20px 20px}}h1{{font-size:18px;margin:18px 20px 0;\
         color:#002050}}@media (prefers-color-scheme:dark){{body{{background:linear-gradient(160deg,#0F1923,#132744);\
         color:#e5e7eb}}main{{background:#1b2533}}h1{{color:#58A6FF}}}}\
         </style></head><body><main><header>EVE · CONSTRUCTO AI</header><h1>{titre}</h1><p>{texte}</p></main></body></html>"
    )
}

/// Un aperçu : 25 Mo au plus.
const APERCU_MAX: u64 = 25 * 1024 * 1024;

/// La sorte d'un fichier qui s'affiche en aperçu, d'après son extension ; `None` : il ne s'affiche pas.
pub fn sorte_d_apercu(chemin: &Path) -> Option<&'static str> {
    let ext = chemin.extension()?.to_str()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "pdf" => "application/pdf",
        "html" | "htm" => "text/html; charset=utf-8",
        _ => return None,
    })
}

/// Sert un fichier du poste en aperçu (la carte « apercu » de la fenêtre). Une image, un PDF ou une page HTML ; la page
/// s'ouvre dans un bac à sable (une origine à part, sans accès à Eve), le SVG aussi, scripts compris.
fn servir_apercu(mut flux: TcpStream, chemin: &str) -> io::Result<()> {
    let chemin = Path::new(chemin);
    let Some(sorte) = sorte_d_apercu(chemin) else {
        return repondre(
            &mut flux,
            403,
            "text/plain; charset=utf-8",
            "Ce fichier ne s'affiche pas en aperçu.".as_bytes(),
        );
    };
    let taille = fs::metadata(chemin).map(|m| if m.is_file() { m.len() } else { u64::MAX });
    match taille {
        Ok(t) if t <= APERCU_MAX => {}
        Ok(u64::MAX) | Err(_) => {
            return repondre(&mut flux, 404, "text/plain; charset=utf-8", "Introuvable.".as_bytes());
        }
        Ok(_) => return repondre(&mut flux, 413, "text/plain; charset=utf-8", "Trop gros pour un aperçu.".as_bytes()),
    }
    let corps = fs::read(chemin)?;
    let politique = matches!(sorte, "text/html; charset=utf-8" | "image/svg+xml").then_some("sandbox allow-scripts");
    flux.write_all(entete_http(200, sorte, corps.len(), politique).as_bytes())?;
    flux.write_all(&corps)?;
    flux.flush()
}

fn entete_http(code: u16, sorte: &str, longueur: usize, politique: Option<&str>) -> String {
    let raison = match code {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        413 => "Payload Too Large",
        _ => "Error",
    };
    let politique = politique.map(|p| format!("Content-Security-Policy: {p}\r\n")).unwrap_or_default();
    format!(
        "HTTP/1.1 {code} {raison}\r\nContent-Type: {sorte}\r\nContent-Length: {longueur}\r\nCache-Control: no-store\r\n\
         X-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\n{politique}Connection: close\r\n\r\n"
    )
}

fn repondre(flux: &mut TcpStream, code: u16, sorte: &str, corps: &[u8]) -> io::Result<()> {
    flux.write_all(entete_http(code, sorte, corps.len(), None).as_bytes())?;
    flux.write_all(corps)?;
    flux.flush()
}

fn lire_corps(flux: &mut TcpStream, req: &Requete, max: u64) -> io::Result<Vec<u8>> {
    let n = req.longueur();
    if n > max {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "corps trop gros"));
    }
    let n = n as usize;
    let mut corps = req.reste.clone();
    corps.truncate(n);
    let mut morceau = vec![0u8; 16 * 1024];
    while corps.len() < n {
        let k = flux.read(&mut morceau[..(n - corps.len()).min(16 * 1024)])?;
        if k == 0 {
            break;
        }
        corps.extend_from_slice(&morceau[..k]);
    }
    Ok(corps)
}

/// Le flux d'état (Server-Sent Events) : l'état courant d'abord, puis chaque changement — les états trop rapprochés se
/// fondent dans le dernier —, et un battement pour voir partir le client.
fn flux_d_etat(mut flux: TcpStream, ecran: &Ecran) -> io::Result<()> {
    // Une page gelée qui ne lit plus ne bloque pas ce fil indéfiniment : elle compte comme partie.
    flux.set_write_timeout(Some(Duration::from_secs(15)))?;
    flux.write_all(
        b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream; charset=utf-8\r\nCache-Control: no-store\r\n\
          X-Content-Type-Options: nosniff\r\nConnection: keep-alive\r\n\r\n",
    )?;
    let (tx, rx) = mpsc::channel::<String>();
    let initial = ecran.partage.etat.lock().map(|e| e.clone()).unwrap_or_default();
    if let Ok(mut a) = ecran.partage.abonnes.lock() {
        a.push(tx);
    }
    ecran.partage.actifs.fetch_add(1, Ordering::SeqCst);
    let resultat = (|| -> io::Result<()> {
        evenement(&mut flux, &initial)?;
        loop {
            match rx.recv_timeout(BATTEMENT) {
                Ok(mut etat) => {
                    while let Ok(plus_recent) = rx.try_recv() {
                        etat = plus_recent;
                    }
                    evenement(&mut flux, &etat)?;
                }
                Err(RecvTimeoutError::Timeout) => {
                    flux.write_all(b": battement\n\n")?;
                    flux.flush()?;
                }
                Err(RecvTimeoutError::Disconnected) => return Ok(()),
            }
        }
    })();
    ecran.partage.actifs.fetch_sub(1, Ordering::SeqCst);
    resultat
}

/// Un événement : l'état est un JSON d'une seule ligne (`serde_json::to_string`), donc un seul champ `data`.
fn evenement(flux: &mut TcpStream, donnees: &str) -> io::Result<()> {
    flux.write_all(b"data: ")?;
    flux.write_all(donnees.as_bytes())?;
    flux.write_all(b"\n\n")?;
    flux.flush()
}

/// Une pièce jointe de la page : le corps de la requête est le fichier, joint comme ceux de la fenêtre native
/// ([`joindre`]) ; un refus répond son texte.
fn recevoir_piece(mut flux: TcpStream, req: &Requete, rapport: &Sender<Evt>) -> io::Result<()> {
    let n = req.longueur();
    flux.set_read_timeout(Some(Duration::from_secs(30)))?;
    let deja = &req.reste[..req.reste.len().min(n as usize)];
    let corps = deja.chain((&mut flux).take(n - deja.len() as u64));
    match joindre(&req.parametre("nom").unwrap_or_default(), n, corps, rapport) {
        Ok(()) => repondre(&mut flux, 200, "application/json", br#"{"ok":true}"#),
        Err(refus) => {
            let code = if matches!(refus, PieceRefusee::Echec(_)) { 400 } else { 413 };
            repondre(&mut flux, code, "text/plain; charset=utf-8", refus.to_string().as_bytes())
        }
    }
}

/// Pourquoi une pièce n'est pas jointe ; chacune porte le nom du fichier, et son texte est celui que la personne lit.
#[derive(Debug, PartialEq)]
pub(crate) enum PieceRefusee {
    /// Vide, ou de plus de 40 Mo : rien n'est écrit.
    Vide(String),
    TropGrosse(String),
    /// Elle ne s'est pas lue, ou pas écrite, en entier : ce qui en était écrit s'efface.
    Echec(String),
}

impl PieceRefusee {
    /// Un échec, et sa raison au journal : la personne ne lit que le nom du fichier.
    pub(crate) fn echec(nom: &str, pourquoi: impl std::fmt::Display) -> PieceRefusee {
        crate::journal::ecrire(&format!("pièce jointe « {nom} » refusée : {pourquoi}"));
        PieceRefusee::Echec(nom.to_string())
    }
}

impl std::fmt::Display for PieceRefusee {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PieceRefusee::Vide(nom) => write!(f, "« {nom} » est vide : non joint."),
            PieceRefusee::TropGrosse(nom) => write!(f, "« {nom} » dépasse 40 Mo : non joint."),
            PieceRefusee::Echec(nom) => write!(f, "« {nom} » n'a pas pu être joint."),
        }
    }
}

/// Une pièce jointe, d'où qu'elle vienne — la page l'envoie (`POST /piece`), la fenêtre native la lit sur le disque :
/// ses `taille` octets, lus dans `source`, écrits dans `pieces\AAAA-MM-JJ\` du dossier local (jamais dans le dossier
/// partagé) sous un nom sûr et libre, puis annoncés au chef. Vide ou de plus de 40 Mo, elle est refusée sans rien
/// écrire.
pub(crate) fn joindre(nom: &str, taille: u64, source: impl Read, rapport: &Sender<Evt>) -> Result<(), PieceRefusee> {
    joindre_dans(&dossier_local(), nom, taille, source, rapport)
}

/// [`joindre`], dans le dossier local `local` (un essai a le sien).
fn joindre_dans(
    local: &Path,
    nom: &str,
    taille: u64,
    source: impl Read,
    rapport: &Sender<Evt>,
) -> Result<(), PieceRefusee> {
    if taille == 0 {
        return Err(PieceRefusee::Vide(nom.to_string()));
    }
    if taille > PIECE_MAX {
        return Err(PieceRefusee::TropGrosse(nom.to_string()));
    }
    let dossier = local.join("pieces").join(crate::journal::date());
    if let Err(e) = fs::create_dir_all(&dossier) {
        return Err(PieceRefusee::echec(nom, e));
    }
    let chemin = libre(&dossier, &nom_sur(nom));
    match File::create(&chemin).and_then(|mut fichier| io::copy(&mut source.take(taille), &mut fichier)) {
        Ok(n) if n == taille => {
            let nom = chemin.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let _ = rapport.send(Evt::Piece { nom, chemin, taille });
            Ok(())
        }
        copie => {
            let _ = fs::remove_file(&chemin);
            Err(PieceRefusee::echec(nom, copie.map_or_else(|e| e.to_string(), |n| format!("{n} octets sur {taille}"))))
        }
    }
}

/// Une numérisation de la caméra : ses pages en JPEG, dans une seule requête (chacune précédée de sa longueur),
/// réunies en un PDF dans `pieces\AAAA-MM-JJ\` du dossier local, puis annoncées au chef avec leur but — `classer` (la
/// ranger dans `Scan\` et la confier à l'Adjointe) ou `joindre` (la joindre à la prochaine demande).
fn recevoir_scan(mut flux: TcpStream, req: &Requete, rapport: &Sender<Evt>) -> io::Result<()> {
    let but = match req.parametre("but").as_deref() {
        Some(b @ ("classer" | "joindre")) => b.to_string(),
        _ => return repondre(&mut flux, 400, "text/plain; charset=utf-8", "But inconnu.".as_bytes()),
    };
    let sorte = match req.parametre("sorte").as_deref() {
        Some(s @ ("document" | "couleur" | "photo")) => s.to_string(),
        _ => "document".to_string(),
    };
    let n = req.longueur();
    if n == 0 || n > PIECE_MAX {
        return repondre(
            &mut flux,
            413,
            "text/plain; charset=utf-8",
            "Numérisation vide ou de plus de 40 Mo.".as_bytes(),
        );
    }
    flux.set_read_timeout(Some(Duration::from_secs(30)))?;
    let corps = lire_corps(&mut flux, req, PIECE_MAX)?;
    if corps.len() as u64 != n {
        return repondre(&mut flux, 400, "text/plain; charset=utf-8", "Numérisation incomplète.".as_bytes());
    }
    let pdf = eve::pdf::lire_trames(&corps).and_then(|pages| {
        Ok((pages.len(), eve::pdf::assembler(&pages, "Numérisation d'Eve", &crate::journal::date_pdf())?))
    });
    let (pages, pdf) = match pdf {
        Ok(x) => x,
        Err(e) => {
            crate::journal::ecrire(&format!("numérisation refusée : {e}"));
            return repondre(
                &mut flux,
                400,
                "text/plain; charset=utf-8",
                format!("Numérisation refusée : {e}.").as_bytes(),
            );
        }
    };
    let dossier = dossier_local().join("pieces").join(crate::journal::date());
    fs::create_dir_all(&dossier)?;
    let chemin = libre(&dossier, &format!("Numérisation Eve {}.pdf", crate::journal::horodatage()));
    fs::write(&chemin, &pdf)?;
    let nom = chemin.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let taille = pdf.len() as u64;
    let _ = rapport.send(Evt::Scan { nom, chemin, pages, but, sorte, taille });
    let reponse = serde_json::json!({ "ok": true, "pages": pages, "taille": taille }).to_string();
    repondre(&mut flux, 200, "application/json", reponse.as_bytes())
}

/// Un nom de fichier sûr : le dernier élément d'un chemin, sans caractère interdit par Windows ni nom réservé.
fn nom_sur(nom: &str) -> String {
    let base = nom.rsplit(['/', '\\']).next().unwrap_or("");
    let propre: String = base.chars().filter(|c| !c.is_control() && !"<>:\"/\\|?*".contains(*c)).take(120).collect();
    let propre = propre.trim().trim_matches('.').trim().to_string();
    let tige = propre.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserve = ["CON", "PRN", "AUX", "NUL"].contains(&tige.as_str())
        || ((tige.starts_with("COM") || tige.starts_with("LPT")) && tige.len() == 4);
    match (propre.is_empty(), reserve) {
        (true, _) => "piece".into(),
        (false, true) => format!("_{propre}"),
        (false, false) => propre,
    }
}

/// Un chemin libre dans `dossier` : le nom tel quel, sinon « nom (2).ext », « nom (3).ext »…
pub fn libre(dossier: &Path, nom: &str) -> PathBuf {
    let p = dossier.join(nom);
    if !p.exists() {
        return p;
    }
    let (tige, ext) = match nom.rsplit_once('.') {
        Some((t, e)) if !t.is_empty() => (t.to_string(), format!(".{e}")),
        _ => (nom.to_string(), String::new()),
    };
    (2..1000).map(|i| dossier.join(format!("{tige} ({i}){ext}"))).find(|p| !p.exists()).unwrap_or(p)
}

/// Le décodage d'un paramètre d'adresse (`%C3%A9` → é, `+` → espace).
pub fn decoder(s: &str) -> String {
    let o = s.as_bytes();
    let mut sortie = Vec::with_capacity(o.len());
    let mut i = 0;
    while i < o.len() {
        match o[i] {
            b'+' => {
                sortie.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < o.len() => {
                match u8::from_str_radix(std::str::from_utf8(&o[i + 1..i + 3]).unwrap_or("zz"), 16) {
                    Ok(b) => {
                        sortie.push(b);
                        i += 3;
                    }
                    Err(_) => {
                        sortie.push(b'%');
                        i += 1;
                    }
                }
            }
            b => {
                sortie.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&sortie).into_owned()
}

#[cfg(test)]
mod essais {
    use super::*;

    /// Le nom affiché remplace celui de l'Adjointe dans le titre — celui que `fenetre` cherche — et sous « Eve ».
    #[test]
    fn la_page_porte_le_nom_affiche() {
        let p = page_servie("J", "A", "Designer");
        assert!(p.contains(&format!("<title>{}</title>", crate::fenetre::titre_pour("Designer"))));
        assert!(p.contains("id=\"sous\">Designer</span>"));
        assert!(!p.contains("Adjointe · Entrepreneur Général</span>") && !p.contains("<title>Eve — Adjointe"));
        let p = page_servie("J", "A", "Adjointe");
        assert!(p.contains(&format!("<title>{}</title>", crate::fenetre::titre_pour("Adjointe"))));
        assert!(p.contains("id=\"sous\">Adjointe · Entrepreneur Général</span>"));
        assert!(page_servie("J", "A", "A<b>").contains("<title>Eve — A&lt;b&gt;</title>"));
    }

    fn demander(port: u16, cible: &str) -> String {
        let mut f = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(f, "GET {cible} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n").unwrap();
        let mut r = Vec::new();
        f.read_to_end(&mut r).unwrap();
        String::from_utf8_lossy(&r).into_owned()
    }

    #[test]
    fn un_apercu_se_sert_avec_son_jeton_seulement() {
        let dossier = std::env::temp_dir().join(format!("eve-apercu-{}", std::process::id()));
        fs::create_dir_all(&dossier).unwrap();
        let page = dossier.join("plan.html");
        fs::write(&page, "<p>plan</p>").unwrap();
        let (envoi, _recu) = mpsc::channel();
        let port = 47_900 + (std::process::id() % 90) as u16;
        let ecran = demarrer(port, "jeton-page".into(), envoi).unwrap();
        let chemin = page.to_string_lossy().replace('\\', "/");

        let r = demander(port, &format!("/apercu?a={}&chemin={chemin}", ecran.jeton_apercu));
        assert!(r.starts_with("HTTP/1.1 200"), "{r}");
        assert!(r.contains("Content-Security-Policy: sandbox allow-scripts"), "{r}");
        assert!(r.ends_with("<p>plan</p>"), "{r}");
        // Le jeton de la page ne sert pas d'aperçu, et l'inverse.
        assert!(demander(port, &format!("/apercu?a=jeton-page&chemin={chemin}")).starts_with("HTTP/1.1 404"));
        assert!(demander(port, &format!("/?t={}", ecran.jeton_apercu)).starts_with("HTTP/1.1 403"));
        // Un fichier qui ne s'affiche pas en aperçu.
        let env = dossier.join("acces_base.env");
        fs::write(&env, "EG_JETON=x").unwrap();
        let r = demander(port, &format!("/apercu?a={}&chemin={}", ecran.jeton_apercu, env.to_string_lossy()));
        assert!(r.starts_with("HTTP/1.1 403") && !r.contains("EG_JETON"), "{r}");
        let _ = fs::remove_dir_all(&dossier);
    }

    /// Le retour de la connexion à Constructo AI : sans jeton de la vitrine, il va au chef tel quel (le chef vérifie son
    /// `state`), et la page rendue ne répète rien de ce qu'elle a reçu.
    #[test]
    fn le_retour_de_connexion_va_au_chef_sans_se_repeter() {
        let (envoi, recu) = mpsc::channel();
        let port = 48_100 + (std::process::id() % 90) as u16;
        let _ecran = demarrer(port, "jeton-page".into(), envoi).unwrap();
        let r = demander(
            port,
            "/connexion/retour?code=code-secret-1&state=etat-secret-2&iss=http%3A%2F%2F127.0.0.1%3A47997",
        );
        assert!(r.starts_with("HTTP/1.1 200"), "{r}");
        assert!(r.contains("Content-Security-Policy: default-src 'none'"), "{r}");
        assert!(
            r.contains("Autorisation reçue") && !r.contains("code-secret-1") && !r.contains("etat-secret-2"),
            "{r}"
        );
        match recu.recv_timeout(Duration::from_secs(5)) {
            Ok(Evt::Retour(retour)) => {
                assert_eq!(
                    (retour.code.as_str(), retour.etat_oauth.as_str(), retour.emetteur.as_str()),
                    ("code-secret-1", "etat-secret-2", "http://127.0.0.1:47997")
                );
            }
            _ => panic!("le retour n'est pas allé au chef"),
        }
        let refus = demander(port, "/connexion/retour?error=access_denied&state=x");
        assert!(refus.contains("La connexion n'a pas abouti"), "{refus}");
    }

    /// Le geste de `eve --mettre-a-jour` arrive au chef comme celui de la page, avec le jeton seulement ; personne sur
    /// le port : `ConnectionRefused`, que `mise_a_jour.rs` lit comme « aucune Eve ne tourne ».
    #[test]
    fn un_geste_arrive_au_chef_avec_son_jeton() {
        let (envoi, recu) = mpsc::channel();
        let port = 48_200 + (std::process::id() % 90) as u16;
        let _ecran = demarrer(port, "jeton-page".into(), envoi).unwrap();
        let valeur = serde_json::json!({ "vers": "abc1234", "de": "def5678", "sorte": "mise_a_jour" });
        geste_a_l_eve(port, "jeton-page", "mettre_a_jour", &valeur).unwrap();
        match recu.recv_timeout(Duration::from_secs(5)) {
            Ok(Evt::Ui { action, valeur: v }) => assert_eq!((action.as_str(), v), ("mettre_a_jour", valeur)),
            _ => panic!("le geste n'est pas allé au chef"),
        }
        assert!(geste_a_l_eve(port, "mauvais-jeton", "quitter", &Value::Null).is_err(), "sans le jeton : refusé");
        assert!(recu.try_iter().next().is_none());
        let libre = TcpListener::bind(("127.0.0.1", 0)).unwrap().local_addr().unwrap().port();
        let e = geste_a_l_eve(libre, "j", "quitter", &Value::Null).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::ConnectionRefused, "{e}");
    }

    #[test]
    fn seul_ce_qui_s_affiche_passe_en_apercu() {
        assert_eq!(sorte_d_apercu(Path::new(r"C:\x\plan.SVG")), Some("image/svg+xml"));
        assert_eq!(sorte_d_apercu(Path::new("C:/x/devis.pdf")), Some("application/pdf"));
        assert_eq!(sorte_d_apercu(Path::new("C:/x/page.html")), Some("text/html; charset=utf-8"));
        for refuse in ["C:/x/acces_base.env", "C:/x/programme.exe", "C:/x/notes.txt", "C:/x/sans-extension"] {
            assert_eq!(sorte_d_apercu(Path::new(refuse)), None, "{refuse}");
        }
    }

    #[test]
    fn decoder_un_nom() {
        assert_eq!(decoder("Facture%20Martin%20%C3%A9t%C3%A9.pdf"), "Facture Martin été.pdf");
        assert_eq!(decoder("a+b%"), "a b%");
    }

    #[test]
    fn un_nom_sur() {
        assert_eq!(nom_sur(r"C:\x\..\facture?.pdf"), "facture.pdf");
        assert_eq!(nom_sur("CON.txt"), "_CON.txt");
        assert_eq!(nom_sur("..."), "piece");
    }

    /// Une pièce se joint comme la page et la fenêtre native la joignent : copiée sous un nom sûr et libre, puis
    /// annoncée au chef ; vide, trop grosse ou incomplète, elle est refusée, sans rien annoncer ni laisser derrière.
    #[test]
    fn une_piece_se_joint_ou_se_refuse() {
        let local = std::env::temp_dir().join(format!("eve-pieces-{}", std::process::id()));
        let (envoi, recu) = mpsc::channel();
        let contenu = b"%PDF-1.4 une facture";
        let n = contenu.len() as u64;
        assert_eq!(joindre_dans(&local, r"C:\x\facture?.pdf", n, &contenu[..], &envoi), Ok(()));
        assert_eq!(joindre_dans(&local, "facture.pdf", n, &contenu[..], &envoi), Ok(()));
        let dossier = local.join("pieces").join(crate::journal::date());
        let annonces: Vec<(String, PathBuf, u64)> = recu
            .try_iter()
            .filter_map(|e| match e {
                Evt::Piece { nom, chemin, taille } => Some((nom, chemin, taille)),
                _ => None,
            })
            .collect();
        let noms: Vec<&str> = annonces.iter().map(|(nom, _, _)| nom.as_str()).collect();
        assert_eq!(noms, ["facture.pdf", "facture (2).pdf"], "un nom déjà pris en prend un autre");
        for (_, chemin, taille) in &annonces {
            assert!(chemin.starts_with(&dossier), "{}", chemin.display());
            assert_eq!((fs::read(chemin).unwrap().as_slice(), *taille), (&contenu[..], n));
        }
        let refus = [
            joindre_dans(&local, "vide.txt", 0, &b""[..], &envoi),
            joindre_dans(&local, "gros.bin", PIECE_MAX + 1, io::empty(), &envoi),
            joindre_dans(&local, "coupe.pdf", n + 10, &contenu[..], &envoi),
        ];
        let restes = ["vide.txt", "gros.bin", "coupe.pdf"].map(|f| dossier.join(f).exists());
        let _ = fs::remove_dir_all(&local);
        assert_eq!(
            refus.map(|r| r.map_err(|e| e.to_string())),
            [
                Err("« vide.txt » est vide : non joint.".into()),
                Err("« gros.bin » dépasse 40 Mo : non joint.".into()),
                Err("« coupe.pdf » n'a pas pu être joint.".into()),
            ]
        );
        assert!(recu.try_iter().next().is_none(), "un refus ne s'annonce pas");
        assert_eq!(restes, [false; 3], "un refus ne laisse rien");
    }
}
