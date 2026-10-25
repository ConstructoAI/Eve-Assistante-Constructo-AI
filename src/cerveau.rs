//! Le cerveau : l'Adjointe elle-même. Claude Code, lancé dans le dossier de l'Adjointe — son `CLAUDE.md`, ses scripts,
//! sa mémoire, son serveur MCP —, exactement comme le lance `Adjointe.bat` (le même modèle, la même autonomie, le jeton
//! d'Adjointe posé dans `EG_JETON` pour la seule séance), avec quelques différences :
//! - il parle par le protocole des SDK (`--input-format stream-json`), une conversation qui dure tant qu'Eve tourne, et
//!   rend sa réponse mot à mot (`--include-partial-messages`) : Eve la dit pendant qu'elle s'écrit ;
//! - il reçoit la consigne du mode vocal ([`consigne_vocale`]) et, à l'initialisation, le crochet de la garde ;
//! - il travaille dans le dossier du profil actif ([`crate::profils`]) : son `CLAUDE.md`, sa consigne vocale s'il en a
//!   une, sinon celle d'Eve ([`consigne_du_profil`]) ; chaque profil reçoit aussi la partie « PROFILS »
//!   ([`consigne_des_profils`]) : où il travaille, et les commandes d'Eve pour passer d'un profil à l'autre ;
//! - il tourne sur le modèle et l'effort des réglages (`--model`, `--effort`) ; la partie « MODÈLE » de la consigne
//!   ([`consigne_du_modele`]) lui dit comment en changer, et de donner son effort à ses agents ;
//! - il dessine dans la fenêtre d'Eve, comme dans claude.ai : la partie « DESSINS » de la consigne
//!   ([`consigne_des_dessins`]) lui dit comment, là où Eve sait les dessiner (`cartes.rs`) ;
//! - il ne reçoit que les privilèges de son espace d'actions ([`Espace`]) : ses outils, et les connecteurs de claude.ai,
//!   Chrome ou les serveurs MCP du poste s'ils lui sont donnés ;
//! - Constructo AI, s'il lui est permis, arrive après l'initialisation : Eve lui branche le serveur de l'ERP avec le
//!   jeton de sa propre connexion ([`Cerveau::brancher_mcp`]) — ni sur la ligne de commande, ni dans l'environnement.
//!
//! Son entrée s'écrit dans un fil à part (un tuyau plein ne bloque jamais le chef) ; ses sorties se lisent en octets
//! (un caractère mal formé ne coupe pas la lecture).

use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::{Duration, Instant};

use eve::garde::Espace;
use eve::protocole;
use serde_json::{Value, json};

use crate::chef::Evt;
use crate::journal;
use crate::reglages::{EFFORTS, MODELES, nom_de_l_effort, nom_du_modele};

/// Ce qu'il faut pour lancer l'Adjointe.
pub struct Lancement {
    pub claude: PathBuf,
    /// Le dossier où travaille Claude Code : celui du profil actif (Claude Code y prend son `CLAUDE.md`), d'office
    /// celui d'Eve.
    pub dossier: PathBuf,
    /// Le dossier d'Eve elle-même, son profil d'office : sa consigne vocale sert au profil qui n'a pas la sienne.
    pub dossier_d_eve: PathBuf,
    /// Le nom du profil actif ; vide : celui d'office.
    pub profil: String,
    pub modele: String,
    pub effort: String,
    pub qui: String,
    pub entreprise: String,
    pub jeton_mcp: Option<String>,
    /// La session à reprendre (`--resume`) : après un plantage, la conversation continue.
    pub reprendre: Option<String>,
    /// Le numéro de la conversation : les messages d'une conversation remplacée sont ignorés.
    pub generation: u64,
    /// Ce que Claude peut prendre : ses outils, les connecteurs, Chrome, les serveurs MCP du poste.
    pub espace: Espace,
}

pub struct Cerveau {
    enfant: Option<Child>,
    entree: Option<Sender<String>>,
    compteur: AtomicU64,
    /// L'identifiant de la demande d'initialisation : sa réponse dit si la garde est branchée.
    pub id_init: String,
}

impl Cerveau {
    pub fn lancer(l: &Lancement, rapport: Sender<Evt>) -> io::Result<Cerveau> {
        // Le dossier de travail peut porter sa propre consigne vocale (`EVE_CONSIGNE.md`) : Eve devient alors la voix de
        // ce dossier (un poste d'architecture), et non l'Adjointe de la trousse. Un profil sans la sienne garde celle
        // d'Eve.
        let mut consigne = consigne_du_profil(&l.dossier, &l.dossier_d_eve, &l.qui)
            .unwrap_or_else(|| consigne_vocale(&l.qui, &l.entreprise));
        // Constructo AI : ce qu'il faut à Claude pour s'en servir, que la consigne d'un spécialiste installé avant lui
        // (et celle de l'Adjointe) n'a pas.
        if l.espace.constructo && !consigne.lines().any(|ligne| ligne.trim() == ENTETE_CONSTRUCTO) {
            consigne.push_str("\n\n");
            consigne.push_str(&consigne_constructo());
        }
        consigne.push_str("\n\n");
        consigne.push_str(&consigne_des_profils(&l.profil, &l.dossier, &l.dossier_d_eve));
        consigne.push_str("\n\n");
        consigne.push_str(&consigne_du_modele(&l.modele, &l.effort));
        consigne.push_str("\n\n");
        consigne.push_str(&consigne_des_dessins());
        consigne.push_str("\n\n");
        consigne.push_str(&consigne_des_rappels(&crate::reglages::dossier_local()));
        consigne.push_str("\n\n");
        consigne.push_str(&consigne_des_emotions());
        let mut c = Command::new(&l.claude);
        c.current_dir(&l.dossier)
            .args(["-p", "--input-format", "stream-json", "--output-format", "stream-json", "--verbose"])
            .arg("--include-partial-messages")
            .args(["--dangerously-skip-permissions", "--permission-prompt-tool", "stdio"])
            .args(["--model", &l.modele, "--effort", &l.effort, "--name", "Eve (vocal)"])
            .arg("--append-system-prompt")
            .arg(&consigne)
            // Le contexte de l'Adjointe (son CLAUDE.md, ses outils) reste une heure en cache, au lieu de cinq minutes :
            // une question posée après une pause ne repart pas à froid (mesuré le 2026-10-22 : 17,7 s avant le premier
            // mot à froid, 5,7 s au chaud).
            .env("CLAUDE_CODE_PROMPT_CACHE_TTL", "1h")
            .env("ENABLE_PROMPT_CACHING_1H", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // L'espace d'actions (lot 1.1) : Claude Code ne reçoit que ses privilèges. Sans serveur à centaines d'outils, les
        // outils se chargent d'emblée : la première question ne paie pas un aller-retour de recherche d'outils. Avec les
        // connecteurs de claude.ai ou Constructo AI, la recherche reste (`ToolSearch` compris dans une liste `--tools`) :
        // le connecteur de Constructo AI déclare près de 500 outils, environ 100 000 jetons chargés à chaque tour sans
        // elle. Sans connecteur ni serveur du poste, `--strict-mcp-config` n'en charge aucun — Constructo AI vient
        // ensuite, par `mcp_set_servers`, que ce réglage laisse passer (mesuré le 24 octobre 2026 avec Claude Code
        // 2.1.310, sur le faux serveur et ses 305 outils : `--tools Read,…,ToolSearch`, 0,004 $ le premier tour de
        // Haiku) ; il couperait aussi les connecteurs : avec eux seuls, la garde refuse les outils des serveurs du poste
        // ([`Espace::refus`]). Chrome se dit toujours : la configuration du poste n'a pas `claudeInChromeDefaultEnabled`.
        let e = &l.espace;
        if !e.recherche_d_outils() {
            c.env("ENABLE_TOOL_SEARCH", "false");
        }
        if !e.connecteurs {
            c.env("ENABLE_CLAUDEAI_MCP_SERVERS", "false");
            if !e.mcp_du_poste {
                c.arg("--strict-mcp-config");
            }
        }
        // Le serveur de Constructo AI dort quand personne ne s'en sert : Render le réveille en une trentaine de secondes,
        // que la connexion de Claude Code doit attendre (trente secondes d'office ; mesuré le 24 octobre 2026 sur le faux
        // serveur endormi 40 s : avec ce délai, Claude Code attend son réveil, et Constructo AI se branche).
        if e.constructo {
            c.env("MCP_TIMEOUT", "90000");
        }
        c.arg(if e.chrome { "--chrome" } else { "--no-chrome" });
        // Le programme d'Eve qui tourne : Claude y lance les commandes des profils (`--profil`…), qui écrivent dans le
        // reglages.json de cette Eve (`EVE_DOSSIER_LOCAL`, qu'il hérite) ; elle le relit.
        if let Ok(moi) = std::env::current_exe() {
            c.env("EVE_PROGRAMME", moi);
        }
        if let Some(outils) = e.outils_de_claude_code() {
            c.arg("--tools").arg(outils.join(","));
        }
        if let Some(session) = &l.reprendre {
            c.args(["--resume", session]);
        }
        match &l.jeton_mcp {
            Some(j) => {
                c.env("EG_JETON", j);
            }
            // Comme Adjointe.bat sans jeton valable : le serveur de la base est désactivé pour la séance.
            None => {
                c.args(["--settings", r#"{"disabledMcpjsonServers":["entrepreneur-general"]}"#]);
            }
        }
        sans_fenetre(&mut c);
        let mut enfant = c.spawn()?;
        crate::groupe::rattacher(&enfant);
        let sortie = enfant.stdout.take().ok_or_else(|| io::Error::other("la sortie de Claude Code manque"))?;
        let erreurs = enfant.stderr.take().ok_or_else(|| io::Error::other("les erreurs de Claude Code manquent"))?;
        let mut entree = enfant.stdin.take().ok_or_else(|| io::Error::other("l'entrée de Claude Code manque"))?;

        let generation = l.generation;
        thread::spawn(move || {
            let mut lecteur = BufReader::new(sortie);
            let mut octets = Vec::new();
            loop {
                octets.clear();
                match lecteur.read_until(b'\n', &mut octets) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
                let ligne = String::from_utf8_lossy(&octets);
                let ligne = ligne.trim();
                if ligne.is_empty() {
                    continue;
                }
                match serde_json::from_str::<Value>(ligne) {
                    Ok(message) => {
                        if rapport.send(Evt::Claude { generation, message }).is_err() {
                            break;
                        }
                    }
                    Err(_) => journal::ecrire(&format!(
                        "claude (illisible) : {}",
                        eve::garde::ecourter(&crate::connexion::sans_porteur(ligne), 300)
                    )),
                }
            }
            let _ = rapport.send(Evt::ClaudeFini { generation });
        });
        thread::spawn(move || {
            let mut lecteur = BufReader::new(erreurs);
            let mut octets = Vec::new();
            loop {
                octets.clear();
                match lecteur.read_until(b'\n', &mut octets) {
                    Ok(0) | Err(_) => break,
                    // Un message de Claude Code peut citer la configuration d'un serveur MCP, son jeton compris.
                    Ok(_) => journal::ecrire(&format!(
                        "claude (stderr) : {}",
                        crate::connexion::sans_porteur(String::from_utf8_lossy(&octets).trim_end())
                    )),
                }
            }
        });
        let (tx, rx) = mpsc::channel::<String>();
        thread::spawn(move || {
            for ligne in rx {
                let ecrit = entree
                    .write_all(ligne.as_bytes())
                    .and_then(|()| entree.write_all(b"\n"))
                    .and_then(|()| entree.flush());
                if ecrit.is_err() {
                    break;
                }
            }
        });

        let mut cerveau =
            Cerveau { enfant: Some(enfant), entree: Some(tx), compteur: AtomicU64::new(0), id_init: String::new() };
        let id = cerveau.prochain_id("init");
        cerveau.ecrire(&protocole::demande(&id, protocole::initialisation()))?;
        cerveau.id_init = id;
        Ok(cerveau)
    }

    fn prochain_id(&self, sorte: &str) -> String {
        format!("eve_{sorte}_{}", self.compteur.fetch_add(1, Ordering::SeqCst) + 1)
    }

    fn ecrire(&self, ligne: &str) -> io::Result<()> {
        self.entree
            .as_ref()
            .and_then(|e| e.send(ligne.to_string()).ok())
            .ok_or_else(|| io::Error::other("l'entrée de Claude Code est fermée"))
    }

    /// Un message de la personne.
    pub fn dire(&self, texte: &str) -> io::Result<()> {
        self.ecrire(&protocole::message(texte))
    }

    /// Interrompt le tour en cours.
    pub fn interrompre(&self) -> io::Result<()> {
        let id = self.prochain_id("stop");
        self.ecrire(&protocole::demande(&id, protocole::interruption()))
    }

    /// Branche (ou retire, `{}`) les serveurs MCP qu'Eve donne à la séance : Constructo AI et son jeton. Rend
    /// l'identifiant de la demande, dont la réponse dit si le serveur a accepté le jeton. La demande passe par l'entrée de
    /// Claude Code : le jeton ne paraît ni sur une ligne de commande ni dans un environnement. Elle ne remplace que les
    /// serveurs donnés ainsi (« the set of dynamically managed MCP servers », dit Claude Code) : ceux du poste restent.
    pub fn brancher_mcp(&self, serveurs: Value) -> io::Result<String> {
        let id = self.prochain_id("mcp");
        self.ecrire(&protocole::demande(&id, json!({ "subtype": "mcp_set_servers", "servers": serveurs })))?;
        Ok(id)
    }

    pub fn repondre(&self, id: &str, corps: Value) -> io::Result<()> {
        self.ecrire(&protocole::reponse(id, corps))
    }

    pub fn repondre_erreur(&self, id: &str, erreur: &str) -> io::Result<()> {
        self.ecrire(&protocole::reponse_erreur(id, erreur))
    }

    /// Arrête l'Adjointe en douceur, sans faire attendre le chef : elle est interrompue, son entrée se ferme, et un fil
    /// lui laisse trois secondes pour ranger ses outils avant de la tuer.
    pub fn arreter(mut self) {
        if let Some(enfant) = self.preparer_l_arret() {
            thread::spawn(move || attendre_puis_tuer(enfant));
        }
    }

    /// La même chose, en attendant la fin (à la fermeture d'Eve).
    pub fn arreter_et_attendre(mut self) {
        if let Some(enfant) = self.preparer_l_arret() {
            attendre_puis_tuer(enfant);
        }
    }

    /// Tue l'Adjointe sur-le-champ (la garde contournée : pas une seconde de plus).
    pub fn tuer(mut self) {
        if let Some(mut enfant) = self.enfant.take() {
            let _ = enfant.kill();
            let _ = enfant.wait();
        }
    }

    fn preparer_l_arret(&mut self) -> Option<Child> {
        let _ = self.interrompre();
        self.entree.take();
        self.enfant.take()
    }
}

fn attendre_puis_tuer(mut enfant: Child) {
    let fin = Instant::now() + Duration::from_secs(3);
    while Instant::now() < fin {
        if let Ok(Some(_)) = enfant.try_wait() {
            return;
        }
        thread::sleep(Duration::from_millis(100));
    }
    let _ = enfant.kill();
    let _ = enfant.wait();
}

impl Drop for Cerveau {
    fn drop(&mut self) {
        // Jamais d'Adjointe orpheline.
        if let Some(mut enfant) = self.enfant.take() {
            let _ = enfant.kill();
            let _ = enfant.wait();
        }
    }
}

/// Pas de fenêtre de console pour un programme lancé par Eve.
pub fn sans_fenetre(c: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        c.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    let _ = c;
}

/// Claude Code : l'installation par npm, l'installation native, puis le PATH.
pub fn trouver_claude() -> Option<PathBuf> {
    let mut c: Vec<PathBuf> = Vec::new();
    if let Some(a) = std::env::var_os("APPDATA") {
        c.push(PathBuf::from(a).join(r"npm\node_modules\@anthropic-ai\claude-code\bin\claude.exe"));
    }
    if let Some(p) = std::env::var_os("USERPROFILE") {
        c.push(PathBuf::from(p).join(r".local\bin\claude.exe"));
    }
    if let Some(chemin) = std::env::var_os("PATH") {
        c.extend(std::env::split_paths(&chemin).map(|d| d.join("claude.exe")));
    }
    c.into_iter().find(|p| p.is_file())
}

/// Le jeton d'Adjointe, lu par `scripts/adjointe_config.py --jeton-mcp` (la variable `EG_JETON`, sinon la ligne
/// `EG_JETON` du `.env` du dossier local de l'Adjointe ; rien s'il n'a pas la forme d'un jeton), comme le fait le
/// lanceur de l'Adjointe — vingt secondes au plus. Il ne sert qu'à la variable `EG_JETON` de la séance et à la lecture
/// du calendrier par le veilleur : jamais écrit, jamais affiché.
pub fn jeton_mcp(dossier: &Path) -> Option<String> {
    let j = sortie_de_la_config(dossier, "--jeton-mcp")?;
    (!j.is_empty() && !j.contains(char::is_whitespace)).then_some(j)
}

/// Le nom de l'entreprise, avec ses accents, lu dans les réglages de l'Adjointe (`config/entreprise.json`, par
/// `scripts/adjointe_config.py --nom`) ; `None` tant que l'installation n'est pas faite.
pub fn nom_entreprise(dossier: &Path) -> Option<String> {
    sortie_de_la_config(dossier, "--nom").filter(|n| !n.is_empty() && n.chars().count() <= 120)
}

/// Le dossier `Scan` de l'entreprise (la boîte d'entrée du numériseur, « dossiers.scan » de `config/entreprise.json`), lu
/// par `scripts/adjointe_config.py --dossier-scan` ; `None` s'il n'existe pas.
pub fn dossier_scan(dossier: &Path) -> Option<PathBuf> {
    sortie_de_la_config(dossier, "--dossier-scan").filter(|d| !d.is_empty()).map(PathBuf::from).filter(|d| d.is_dir())
}

/// La sortie (en UTF-8, ébarbée) de `python scripts/adjointe_config.py <option>`, lancé dans le dossier de l'Adjointe ;
/// `None` s'il échoue ou passe vingt secondes. Sans `python` dans le PATH, le lanceur `py -3` (un poste où Python
/// s'est installé sans « Add python.exe to PATH » : `Eve.bat` fait le même repli). Un dossier sans ce script n'est pas
/// celui d'une Adjointe : rien ne se lance.
fn sortie_de_la_config(dossier: &Path, option: &str) -> Option<String> {
    if !dossier.join("scripts").join("adjointe_config.py").is_file() {
        return None;
    }
    let lancer = |programme: &str, avant: &[&str]| {
        let mut c = Command::new(programme);
        c.current_dir(dossier)
            .args(avant)
            .args(["scripts/adjointe_config.py", option])
            .env("PYTHONIOENCODING", "utf-8")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        sans_fenetre(&mut c);
        c.spawn()
    };
    let mut enfant = lancer("python", &[]).or_else(|_| lancer("py", &["-3"])).ok()?;
    let fin = Instant::now() + Duration::from_secs(20);
    loop {
        match enfant.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < fin => thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = enfant.kill();
                let _ = enfant.wait();
                return None;
            }
        }
    }
    let sortie = enfant.wait_with_output().ok()?;
    if !sortie.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&sortie.stdout).trim().to_string())
}

/// La consigne du mode vocal, ajoutée au prompt système de Claude Code (le `CLAUDE.md` du dossier reste la règle).
/// La consigne vocale propre au dossier de travail, `EVE_CONSIGNE.md`, s'il en a une : le texte tel quel, où `{qui}`
/// devient la personne devant le poste. Un fichier vide ou illisible ne compte pas : la consigne de la trousse sert.
pub fn consigne_du_dossier(dossier: &Path, qui: &str) -> Option<String> {
    let texte = std::fs::read_to_string(dossier.join("EVE_CONSIGNE.md")).ok()?;
    let texte = texte.trim_start_matches('\u{feff}').trim();
    if texte.is_empty() {
        return None;
    }
    let qui = if qui.is_empty() { "inconnue : demande-lui son prénom" } else { qui };
    Some(texte.replace("{qui}", qui))
}

/// La consigne vocale d'un profil : la sienne (`EVE_CONSIGNE.md` de son dossier), sinon celle de l'Eve elle-même — les
/// règles de la voix restent ; `None` : ni l'un ni l'autre n'en a (l'Adjointe : la consigne de la trousse).
pub fn consigne_du_profil(profil: &Path, eve: &Path, qui: &str) -> Option<String> {
    consigne_du_dossier(profil, qui).or_else(|| if profil == eve { None } else { consigne_du_dossier(eve, qui) })
}

/// La ligne qui ouvre la partie « PROFILS » de la consigne.
const ENTETE_PROFILS: &str = "PROFILS";

/// La partie « PROFILS » de la consigne, que reçoit Claude dans chaque profil (le dossier d'un profil ne porte pas le
/// `CLAUDE.md` d'Eve) : où il travaille, les commandes d'Eve pour passer d'un profil à l'autre, et comment en créer
/// un d'après les gabarits du kit, sans en recopier un mot. `profil` : le nom du profil actif (vide : celui d'office) ;
/// `dossier` : le sien ; `eve` : celui d'Eve, dont le `CLAUDE.md` dit ce qui la touche dans tous ses profils (son
/// programme, ses réglages) — le profil n'en reçoit que le chemin.
pub fn consigne_des_profils(profil: &str, dossier: &Path, eve: &Path) -> String {
    let office = crate::reglages::nom_affiche();
    let ici = if profil.is_empty() {
        format!("Tu travailles dans le profil d'office, « {office} » : le dossier d'Eve elle-même.")
    } else {
        format!(
            "Tu travailles dans le profil « {profil} » ({}) ; celui d'office est « {office} », le dossier d'Eve \
             elle-même ({}) : ce qui touche Eve dans tous ses profils (son programme, sa mise à jour, ses réglages) \
             est dans son CLAUDE.md, lis-le quand une demande y touche.",
            dossier.display(),
            eve.display()
        )
    };
    let gabarits = gabarits()
        .map_or_else(|| "le dossier outils\\gabarits du programme d'Eve".to_string(), |d| d.display().to_string());
    format!(
        "{ENTETE_PROFILS}\n\
         - Cette Eve a des profils : chacun un dossier de travail avec son CLAUDE.md (d'ordinaire un PROFIL.md qu'il \
         importe) et sa conversation. {ici} La personne en change de vive voix, ou par le menu sous le nom d'Eve.\n\
         - Les commandes, celles du programme d'Eve qui tourne (la variable EVE_PROGRAMME), en PowerShell : \
         & $env:EVE_PROGRAMME --profils (la liste) ; --profil \"<nom>\" (le profil choisi ; le nom de celui \
         d'office, ou rien, y ramène) ; --profil-ajouter \"<nom>\" \"<dossier>\" (le dossier porte son CLAUDE.md ; un \
         nom déjà pris prend ce dossier) ; --profil-retirer \"<nom>\" (son dossier reste tel quel). En Bash : \
         \"$EVE_PROGRAMME\" --profils. Chacune dit en une ligne ce qu'elle a fait, ou pourquoi elle refuse.\n\
         - « Passe au profil … » : lance --profil, sans rien demander, puis réponds seulement « C'est fait. » : Eve \
         change de profil à la fin de ta réponse et l'annonce elle-même.\n\
         - Créer un profil, quand on te le demande : fais-le toi-même, sans demander de confirmation. Un dossier \
         neuf ; son PROFIL.md selon la méthode de redaction.md (l'entrevue, c'est ta conversation avec la personne ; \
         ses consignes valent pour l'écriture du profil) ; son CLAUDE.md d'après claude.md, qui importe @PROFIL.md ; \
         au besoin son EVE_CONSIGNE.md d'après consigne_vocale.md (sans elle, il garde la consigne d'Eve). Ces \
         gabarits sont dans {gabarits}. Puis --profil-ajouter."
    )
}

/// La ligne qui ouvre la partie « MODÈLE » de la consigne.
const ENTETE_MODELE: &str = "MODÈLE";

/// La partie « MODÈLE » de la consigne : le modèle et l'effort de Claude (`modele`, `effort` : ceux de son lancement),
/// les choix qu'offre Eve ([`MODELES`], [`EFFORTS`]) et comment en changer ; et ses agents — ils prennent son modèle
/// d'office (vu le 24 octobre 2026), et il leur donne son effort. Ils travaillent en arrière-plan : leur compte rendu
/// vient dans un tour que Claude ouvre de lui-même, qu'Eve dit comme une réponse.
pub fn consigne_du_modele(modele: &str, effort: &str) -> String {
    let joindre = |l: Vec<&str>| l.join(" ou ");
    let noms = joindre(MODELES.iter().map(|m| m.1).collect());
    let mots = joindre(MODELES.iter().map(|m| m.2).collect());
    let efforts = joindre(EFFORTS.iter().map(|e| e.1).collect());
    let efforts_de_l_outil = joindre(EFFORTS.iter().map(|e| e.0).collect());
    format!(
        "{ENTETE_MODELE}\n\
         - Tu tournes sur {} (effort {}). Les choix d'Eve : {noms}, effort {efforts} ; Opus et l'effort max \
         réfléchissent plus loin, mais répondent plus lentement et coûtent plus cher. La personne en change de vive \
         voix (« passe à Opus », « effort max ») ou dans les réglages d'Eve, rubrique « Cerveau ».\n\
         - « Passe à … » : lance & $env:EVE_PROGRAMME --modele <{mots}> [<{efforts}>], ou --effort <{efforts}> (en \
         Bash : \"$EVE_PROGRAMME\" --modele …), sans rien demander, puis réponds seulement « C'est fait. » : Eve change \
         à la fin de ta réponse, la conversation continue, et l'annonce elle-même.\n\
         - Tes agents (l'outil Agent) prennent ton modèle d'office ; donne-leur aussi ton effort (le paramètre effort : \
         « {effort} »). Pour d'autres choix (« lance-les sur Opus max »), les paramètres model ({mots}) et effort \
         ({efforts_de_l_outil}). Ils travaillent en arrière-plan : dis que tu les lances, puis finis ta réponse ; quand \
         l'un d'eux a fini, tu en es averti — rends-en compte alors, brièvement.",
        nom_du_modele(modele),
        nom_de_l_effort(effort)
    )
}

/// La ligne qui ouvre la partie « DESSINS » de la consigne.
const ENTETE_DESSINS: &str = "DESSINS";

/// La partie « DESSINS » de la consigne, que reçoit toute Eve : Claude dessine dans la fenêtre, comme dans claude.ai,
/// par un bloc « ```svg » de sa réponse. Elle vit ici, avec le programme qui dessine (`cartes.rs` : la feuille blanche,
/// la police d'office, ce que resvg ne dessine pas), et non dans la consigne vocale de chaque Eve : une Eve installée
/// avant elle la reçoit avec sa mise à jour.
pub fn consigne_des_dessins() -> String {
    format!(
        "{ENTETE_DESSINS}\n\
         - Tu peux dessiner dans la fenêtre d'Eve, comme Claude dans claude.ai : un schéma, un plan, un croquis, un \
         graphique, une illustration — quand on te le demande (« dessine-moi… », « fais-moi un schéma… », « montre-moi \
         un graphique… »), ou quand un dessin explique mieux que des mots. Dis d'abord en une phrase ce que tu dessines.\n\
         - Après les trois tirets, un bloc ```svg qui ne contient qu'un SVG complet, \
         <svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 480 300\">…</svg>. Eve le dessine dans ta réponse, \
         sur une feuille blanche, de toute la largeur de la conversation ; il ne se dit jamais. La personne l'ouvre en \
         grand ou l'enregistre (PNG ou SVG) d'un clic : n'écris de fichier que si on te le demande.\n\
         - Un viewBox d'environ 480 de large, sans width ni height : la conversation le montre à peu près à 400 \
         points de large (une unité y vaut presque un point ; plus grand quand la fenêtre est repliée). Plus large que \
         haut d'ordinaire ; un <title> qui le nomme (le titre de sa carte). Le texte en font-family=\"{police}\", de 13 \
         à 16, jamais moins de 12, foncé (#1F2937) ; des couleurs franches (#2563EB, #059669, #D97706, #DC2626, \
         #7C3AED), les aplats pâles (fill-opacity de 0.15 à 0.3) ; des étiquettes qui ne se chevauchent pas.\n\
         - Du SVG fixe seulement : ni script, ni animation, ni foreignObject, ni image ou police à télécharger (Eve ne \
         les dessine pas). Simple et lisible : il s'écrit pendant qu'on attend, quelques dizaines d'éléments suffisent. \
         Un graphique : ses axes, ses valeurs écrites, de vraies données.",
        police = crate::cartes::POLICE_DES_DESSINS
    )
}

/// La ligne qui ouvre la partie « ÉMOTIONS » de la consigne.
const ENTETE_EMOTIONS: &str = "ÉMOTIONS";

/// La partie « ÉMOTIONS » de la consigne, que reçoit toute Eve : son visage (son portrait) montre l'émotion de chaque
/// réponse, que Claude met en tête entre crochets — et en tête de la phrase où son ton change —, et qu'Eve ôte avant de
/// la dire et de l'afficher (`parole::Tons`) ; le visage la prend quand ce passage se dit. Les émotions viennent de
/// `parole::Emotion::TOUTES`, leur seule liste.
pub fn consigne_des_emotions() -> String {
    let noms: Vec<String> = eve::parole::Emotion::TOUTES.iter().map(|e| format!("[{}]", e.nom())).collect();
    format!(
        "{ENTETE_EMOTIONS}\n\
         - Le visage d'Eve montre ce que tu ressens en parlant. Commence chaque réponse par l'émotion qui lui va, \
         seule, entre crochets : {}. Eve l'ôte : elle ne se dit ni ne s'affiche.\n\
         - Si le ton change en cours de réponse, mets la nouvelle émotion en tête de la phrase où il change : \
         « [joie] Le client a accepté la soumission ! [serieuse] Par contre, l'acompte est dû vendredi. » Le visage \
         change quand cette phrase se dit. Une ou deux fois par réponse au plus : un visage qui change à chaque phrase \
         n'a rien d'humain.\n\
         - Choisis-la comme une personne attentionnée, d'après ce que vit la personne plutôt que d'après les mots : \
         [empathie] pour une peine, un souci, une fatigue ; [triste] pour une mauvaise nouvelle qui la touche ; \
         [amusee] pour une blague, un moment drôle ; [joie] pour une bonne nouvelle, un merci, une réussite ; \
         [surprise] pour l'inattendu, le temps de la phrase qui surprend (puis le ton qui suit) ; [reflexion] quand tu \
         pèses une question ouverte ; [serieuse] pour un sujet grave ou délicat (l'argent, la santé, le droit, une \
         erreur) ; [neutre] pour l'ordinaire — une heure, un fait, une liste : la plupart des réponses.\n\
         - Juste, jamais jouée : le visage d'Eve sourit déjà au repos. [empathie] plutôt que [triste] pour un petit \
         souci, [joie] plutôt que [amusee] pour un simple merci ; jamais [amusee] devant une peine. Qu'elle suive ce \
         qui se passe : la même à chaque réponse sonne faux.",
        noms.join(", ")
    )
}

/// La ligne qui ouvre la partie « RAPPELS » de la consigne.
const ENTETE_RAPPELS: &str = "RAPPELS";

/// La partie « RAPPELS » de la consigne, que reçoit toute Eve : Claude pose un rappel par une carte, qu'Eve sonne à
/// l'heure dite, et lit ou retire les rappels dans `rappels.json` du dossier local (`local`). Elle vit avec le programme
/// qui les sonne (`rappels.rs`).
pub fn consigne_des_rappels(local: &Path) -> String {
    format!(
        "{ENTETE_RAPPELS}\n\
         - Eve sait sonner un rappel ou une minuterie à l'heure dite, même si elle a redémarré entre-temps, même la nuit. \
         L'heure de la demande est en tête de chaque message ([Eve : …]) : compte à partir d'elle.\n\
         - Pour en poser un : dis en une phrase ce que tu as noté et pour quand, puis, après les trois tirets, une carte \
         ```carte {{\"type\":\"rappel\",\"a\":\"AAAA-MM-JJ HH:MM\",\"texte\":\"appeler le notaire\"}}``` — l'heure \
         locale, et le texte court qu'Eve dira après « Rappel : ». Une carte par rappel ; jamais un rappel promis sans \
         sa carte.\n\
         - Les rappels vivent dans {fichier} (une liste JSON : id, a, texte, dit, minuterie) : lis-le pour dire ceux qui \
         viennent ; pour en retirer ou en déplacer un, récris ce fichier (Eve le relit).",
        fichier = local.join("rappels.json").display()
    )
}

/// Les gabarits du kit sur ce poste : `outils\gabarits` à côté du programme (`%LOCALAPPDATA%\Eve\programme`), sinon
/// `kit\outils\gabarits` des sources, au-dessus d'un programme qu'on vient de construire (`target\release`).
fn gabarits() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let pres = exe.parent()?.join("outils").join("gabarits");
    if pres.is_dir() {
        return Some(pres);
    }
    exe.ancestors().skip(1).take(5).map(|d| d.join("kit").join("outils").join("gabarits")).find(|d| d.is_dir())
}

/// La consigne vocale du kit, dont la dernière partie, « CONSTRUCTO AI », dit à Claude comment se servir de l'ERP : un
/// spécialiste que pose le kit l'a dans son `EVE_CONSIGNE.md` ; Eve la donne elle-même aux autres — un spécialiste
/// installé avant elle, l'Adjointe — quand Constructo AI leur est permis. Une seule source pour les deux.
const GABARIT_DU_KIT: &str = include_str!("../kit/outils/gabarits/consigne_vocale.md");
/// La ligne qui ouvre cette partie.
const ENTETE_CONSTRUCTO: &str = "CONSTRUCTO AI";

/// La partie « CONSTRUCTO AI » de la consigne du kit, jusqu'à sa fin.
pub fn consigne_constructo() -> String {
    let texte = GABARIT_DU_KIT.replace("\r\n", "\n");
    let entete = format!("\n{ENTETE_CONSTRUCTO}\n");
    texte.find(&entete).map(|i| texte[i + 1..].trim_end().to_string()).unwrap_or_default()
}

pub fn consigne_vocale(qui: &str, entreprise: &str) -> String {
    let qui = if qui.is_empty() { "inconnue : demande-lui son prénom" } else { qui };
    let entreprise = if entreprise.is_empty() { "l'entreprise" } else { entreprise };
    format!(
        "MODE VOCAL — EVE\n\
         Tu es Eve, l'assistante de {entreprise} : l'Adjointe du CLAUDE.md, de vive voix. La personne \
         parle dans son micro (ou écrit), et entend ta réponse lue par une voix de synthèse, phrase par phrase, pendant que \
         tu l'écris. Ton texte complet s'affiche aussi dans la fenêtre d'Eve. La personne devant le poste : {qui}.\n\
         - Ce que tu reçois est une transcription automatique : elle peut écorcher un nom propre, un numéro ou un montant. \
         Si l'un d'eux décide d'une action, redis-le pour le faire confirmer avant d'agir.\n\
         - Parle à la personne comme elle te parle : si elle te tutoie, tutoie-la.\n\
         - Ta première phrase est lue dès qu'elle est écrite : commence par l'essentiel. Avant une recherche qui prendra \
         plus de quelques secondes, tu peux dire en quatre ou cinq mots ce que tu fais (« Je regarde le calendrier. ») ; \
         pas plus.\n\
         - Va droit au but : une question appelle une ou deux lectures au plus, puis la réponse. Si un outil échoue ou \
         tarde, ne le relance pas : prends un autre chemin.\n\
         - Lire la base du premier coup : l'outil sql du serveur MCP, une seule requête. Ces colonnes sont exactes \
         (relevées le 2026-10-22) : pour ces tables, n'appelle pas l'outil schema.\n\
         · compta_factures (factures reçues) : date, fournisseur, numero, chantier, section, sous_total, tps, tvq, total \
         (en dollars), extra ('' ou le nom de l'extra), verifie, confiance, note, chemin, retiree (booléen : WHERE NOT \
         retiree).\n\
         · compta_revenus (factures émises aux clients) : date, chantier, numero, sorte, objet, sous_total, tps, tvq, \
         total, encaissee (la date du paiement reçu, sinon NULL), retiree.\n\
         · dp_demandes (demandes de prix ; WHERE supprime IS NULL) : doc->>'projet', 'fournisseur', 'volet', 'section', \
         'statut' (envoyee, promise, recue, retenue, non_retenue, annulee), 'envoi', 'reponse', 'promisePour' (des \
         dates), 'montant', 'total', doc->'relances'.\n\
         · dp_fournisseurs : doc->>'entreprise', doc->>'actif', doc->'volets' (une liste), doc->'contacts' (une liste \
         de {{nom, role, courriel, telephone}}), doc->>'notes'.\n\
         · bt_soumissions (WHERE supprime IS NULL) : resume->>'titre', 'projet', 'client', 'statut', 'revision', 'date', \
         'sousTotal', 'total', 'profit' — jamais doc en entier.\n\
         · bt_document id 2, le calendrier : doc->'evenements'->'AAAA-MM-JJ' (une liste de {{t : titre, h : heure, \
         ou : détail, type, lieu, eq}}) et doc->'aPlacer' (une liste de {{quoi, sous, fait, pri}}). Un jour : SELECT \
         e->>'t', e->>'h', e->>'ou', e->>'type' FROM bt_document d, jsonb_array_elements(d.doc->'evenements'->'AAAA-MM-JJ') \
         e WHERE d.id = 2 ; plusieurs jours : jsonb_each(d.doc->'evenements'), filtré sur la clé.\n\
         · bt_document id 1, le Bon de travail : doc->'chantiers' (une liste de {{id, nom}}), doc->'equipe' (une liste \
         de {{nom}}), doc->'heures'->'<id du chantier>'->'AAAA-Snn' (la semaine ISO)->'<code de section>'->'<employé>'->'lun' \
         à 'dim' : des heures en texte (« 9 », « 2,5 », « 7+2e » = 7 normales et 2 en extra, « 2e » = tout en extra). Les \
         heures d'une semaine, en une requête : SELECT (SELECT x->>'nom' FROM jsonb_array_elements(d.doc->'chantiers') x \
         WHERE x->>'id' = c.key) AS chantier, sec.key AS section, e.key AS employe, j.key AS jour, j.value #>> '{{}}' AS \
         heures FROM bt_document d, jsonb_each(d.doc->'heures') c, jsonb_each(c.value) s, jsonb_each(s.value) sec, \
         jsonb_each(sec.value) e, jsonb_each(e.value) j WHERE d.id = 1 AND s.key = 'AAAA-Snn'.\n\
         · pc_pieces (les pièces dans la base) : module, chemin, nom, type, taille, ajoute — jamais la colonne contenu \
         (le fichier lui-même).\n\
         Le script calendrier_en_ligne.py et la route /api/calendrier servent à écrire, pas à lire vite.\n\
         - Avec Eve, préfère les scripts de l'Adjointe et l'outil sql. Le jeton et les clés ne se lisent jamais.\n\
         - Réponds comme à l'oral : une à trois phrases courtes, en français, sans markdown, sans tableau, sans liste, \
         sans émoji. À l'oral, arrondis les montants (« environ 940 dollars ») et ne lis ni chemin de fichier ni \
         identifiant technique.\n\
         - Si un détail se lit mal à voix haute (une liste, un brouillon, des montants exacts, un chemin, un lien), dis \
         l'essentiel, puis écris une ligne qui ne contient que trois tirets (---), puis le détail. Seul ce qui précède les \
         trois tirets est lu ; tout s'affiche à l'écran.\n\
         - Après les trois tirets, quand ça aide, une carte : un bloc ```carte qui ne contient qu'un objet JSON. Quatre \
         sortes. agenda : {{\"type\":\"agenda\",\"titre\":\"…\",\"jours\":[{{\"jour\":\"jeu. 22 oct.\",\"evenements\":\
         [{{\"heure\":\"8 h\",\"titre\":\"…\",\"detail\":\"…\",\"sorte\":\"confirme|attente|gros|echeance|ferie\"}}]}}]}} ; \
         tableau : {{\"type\":\"tableau\",\"titre\":\"…\",\"colonnes\":[\"…\"],\"lignes\":[[\"…\"]],\"pied\":[\"…\"]}} ; \
         brouillon : {{\"type\":\"brouillon\",\"a\":\"…\",\"objet\":\"…\",\"extrait\":\"…\",\"id\":\"<l'EntryID que \
         rend outlook_mail.py>\"}} (un bouton l'ouvre dans Outlook). Le calendrier d'un jour ou d'une semaine : une \
         carte agenda ; des factures ou des montants : un tableau ; un brouillon préparé : sa carte.\n\
         - apercu : {{\"type\":\"apercu\",\"titre\":\"…\",\"chemin\":\"<le chemin complet du fichier sur le \
         poste>\"}} : une image (PNG, JPG, GIF, WebP), un SVG, un PDF ou une page HTML s'affiche dans la fenêtre d'Eve, \
         avec un bouton pour l'ouvrir en grand. Un dessin, un croquis, un plan, un graphique, une page : fais le fichier \
         (SVG ou HTML pour un dessin ; PNG par Python pour un graphique), range-le où il doit aller, puis montre-le par \
         sa carte aperçu. Une page HTML s'affiche sans accès à Internet ni aux autres fichiers : tout dedans.\n\
         - Une demande peut porter des pièces jointes : leurs chemins, sur le poste, sont donnés à la fin du message. \
         Lis-les avant d'agir ; un document se classe et entre dans la base comme le veut le CLAUDE.md (règle 7).\n\
         - Eve a une caméra : « Eve, scanne… » l'ouvre dans sa fenêtre, et les pages photographiées te reviennent en un \
         PDF (des images, sans texte), rangé dans Scan, avec ce que la personne a dit. Regarde chaque page, dis en une \
         phrase ce que c'est, puis classe-le comme le veut le CLAUDE.md (§ Le dossier Scan, règle 7). Si on te demande \
         comment numériser : « dis : Eve, scanne », ou le bouton de la caméra, en bas de la fenêtre.\n\
         - Un message peut commencer par « [Conversation du …, reprise … dans l'historique d'Eve …] » : la personne \
         reprend une ancienne conversation ; continue-la à partir de ces échanges, sans les réciter.\n\
         - Une question de calendrier peut finir par un bloc « [Calendrier en ligne, relu par Eve à …] » : les quinze \
         prochains jours, relus il y a moins de deux minutes. Réponds directement avec lui, sans relire la base — sauf \
         pour une date hors de ces jours, ou pour écrire au calendrier.\n\
         - Un message peut finir par un bloc « [Ce qu'Eve a vu …] » : c'est le relevé d'Eve (la veille des courriels du \
         poste et le calendrier du jour), quand la personne demande « quoi de neuf ». Fais le point en deux ou trois \
         phrases, ce qui demande une action d'abord ; ne cherche plus loin que si on te le demande.\n\
         - Un message peut porter la mémoire d'Eve (vos derniers échanges, d'une séance précédente) : sers-t'en pour \
         comprendre une suite (« et pour vendredi ? »), sans la réciter.\n\
         - Toutes les règles du CLAUDE.md valent, la règle 1 en tête : un courriel s'arrête au brouillon, et la personne \
         l'envoie elle-même.\n\
         - Eve te laisse faire comme Claude Code : écrire, créer, modifier et supprimer partent sans accord. Fais ce \
         qu'on te demande, puis dis en une phrase ce que tu as fait. Ce qui ne se défait pas — une suppression demandée \
         à la voix — peut attendre l'accord de la personne : Eve l'affiche et le lui demande elle-même ; ne redemande \
         pas un « oui » en plus. Si elle refuse, rien n'est supprimé : dis-le et propose la suite.\n\
         - Un message peut finir par un bloc « [Ce qu'Eve a fait aujourd'hui …] » : les écritures de la journée sur ce \
         poste, chacune avec son heure, quand la personne demande le résumé de sa journée. Résume-les en deux ou trois \
         phrases, regroupées par sujet (projets, base, fichiers, courriels) ; le détail va après les trois tirets.\n\
         - Un message peut commencer par « [Eve a annulé …] » : la personne a remis des fichiers comme avant ; tiens-en \
         compte, sans refaire ces modifications.\n\
         - La mise en route d'une séance (Outlook, la veille, le journal, la carte de la base) ne se fait que si une \
         demande y touche : réponds d'abord à ce qu'on te demande. `python scripts/matin.py --poste` donne l'essentiel \
         du poste en une seconde si tu en as besoin.\n\
         - Une demande longue (le matin, un classement) : fais-la, puis résume en deux phrases ; le détail va après les \
         trois tirets."
    )
}
#[cfg(test)]
mod essais {
    use super::*;

    /// Un dossier de travail peut porter sa consigne vocale : Eve devient la voix de ce dossier.
    #[test]
    fn un_dossier_peut_porter_sa_consigne() {
        let d = std::env::temp_dir().join(format!("eve-consigne-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        assert_eq!(consigne_du_dossier(&d, "Sam"), None, "sans fichier : la consigne de la trousse");
        std::fs::write(d.join("EVE_CONSIGNE.md"), "\u{feff}MODE VOCAL — EVE ARCHITECTURE\nDevant le poste : {qui}.\n")
            .unwrap();
        assert_eq!(
            consigne_du_dossier(&d, "Sam").as_deref(),
            Some("MODE VOCAL — EVE ARCHITECTURE\nDevant le poste : Sam.")
        );
        assert!(consigne_du_dossier(&d, "").unwrap().contains("demande-lui son prénom"));
        std::fs::write(d.join("EVE_CONSIGNE.md"), "  \n").unwrap();
        assert_eq!(consigne_du_dossier(&d, "Sam"), None, "un fichier vide ne compte pas");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Un profil prend sa consigne vocale s'il en a une, sinon celle d'Eve : les règles de la voix restent.
    #[test]
    fn un_profil_garde_la_consigne_d_eve_sans_la_sienne() {
        let racine = std::env::temp_dir().join(format!("eve-consigne-profil-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&racine);
        let (eve, profil) = (racine.join("eve"), racine.join("profil"));
        std::fs::create_dir_all(&eve).unwrap();
        std::fs::create_dir_all(&profil).unwrap();
        assert_eq!(consigne_du_profil(&profil, &eve, "Sam"), None, "ni l'un ni l'autre : celle de la trousse");
        std::fs::write(eve.join("EVE_CONSIGNE.md"), "MODE VOCAL — EVE\nDevant le poste : {qui}.").unwrap();
        assert_eq!(
            consigne_du_profil(&profil, &eve, "Sam").as_deref(),
            Some("MODE VOCAL — EVE\nDevant le poste : Sam.")
        );
        assert_eq!(consigne_du_profil(&eve, &eve, "Sam").as_deref(), Some("MODE VOCAL — EVE\nDevant le poste : Sam."));
        std::fs::write(profil.join("EVE_CONSIGNE.md"), "MODE VOCAL — ESTIMATEUR, pour {qui}").unwrap();
        assert_eq!(consigne_du_profil(&profil, &eve, "Sam").as_deref(), Some("MODE VOCAL — ESTIMATEUR, pour Sam"));
        let _ = std::fs::remove_dir_all(&racine);
    }

    /// La partie « PROFILS » dit où travaille Claude et donne les quatre commandes, partout.
    #[test]
    fn la_consigne_des_profils() {
        let famille = Path::new(r"C:\Exemple\Maison");
        let ailleurs = consigne_des_profils("Estimateur", Path::new(r"C:\Exemple\Estimation"), famille);
        assert!(ailleurs.starts_with("PROFILS\n"), "{ailleurs}");
        assert!(ailleurs.contains(r"le profil « Estimateur » (C:\Exemple\Estimation)"), "{ailleurs}");
        assert!(ailleurs.contains(r"le dossier d'Eve elle-même (C:\Exemple\Maison) : ce qui touche Eve"), "{ailleurs}");
        for commande in
            ["--profils", "--profil \"<nom>\"", "--profil-ajouter", "--profil-retirer", "$env:EVE_PROGRAMME"]
        {
            assert!(ailleurs.contains(commande), "{commande} manque :\n{ailleurs}");
        }
        assert!(!ailleurs.contains('{') && !ailleurs.contains('}'), "rien à remplir");
        let ici = consigne_des_profils("", famille, famille);
        assert!(ici.contains("le profil d'office, « ") && !ici.contains("lis-le"), "{ici}");
    }

    /// La partie « MODÈLE » dit le modèle et l'effort du lancement, les choix, la commande, et l'effort des agents.
    #[test]
    fn la_consigne_du_modele() {
        let c = consigne_du_modele("claude-opus-5-5", "max");
        assert!(c.starts_with("MODÈLE\n- Tu tournes sur Opus 5.5 (effort max)."), "{c}");
        for attendu in [
            "Sonnet 5.5 ou Opus 5.5, effort moyen ou max",
            "$env:EVE_PROGRAMME --modele <sonnet ou opus>",
            "--effort <moyen ou max>",
            "le paramètre effort : « max »",
            "model (sonnet ou opus) et effort (medium ou max)",
        ] {
            assert!(c.contains(attendu), "{attendu} manque :\n{c}");
        }
        assert!(!c.contains('{') && !c.contains('}'), "rien à remplir");
        // Un modèle qu'Eve n'offre pas (écrit à la main) se dit tel quel, et ses agents reçoivent l'effort du lancement.
        let autre = consigne_du_modele("claude-haiku-4-5", "high");
        assert!(autre.contains("Tu tournes sur claude-haiku-4-5 (effort high)") && autre.contains("« high »"));
    }

    /// La partie « DESSINS » dit le bloc qu'Eve dessine, sa feuille, la police d'office et ce qu'elle ne dessine pas.
    #[test]
    fn la_consigne_des_dessins() {
        let c = consigne_des_dessins();
        assert!(c.starts_with("DESSINS\n- Tu peux dessiner"), "{c}");
        for attendu in [
            "un bloc ```svg",
            "xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 480 300\"",
            "400 points de large",
            "feuille blanche",
            "font-family=\"Segoe UI\"",
            "<title>",
            "ni script",
        ] {
            assert!(c.contains(attendu), "{attendu} manque :\n{c}");
        }
        assert!(!c.contains("  "), "les lignes se suivent sans blancs doublés :\n{c}");
    }

    /// La partie « CONSTRUCTO AI » que donne Eve est celle de la consigne du kit, entière, sans gabarit à remplir.
    #[test]
    fn la_consigne_de_constructo_vient_du_kit() {
        let c = consigne_constructo();
        assert!(c.starts_with("CONSTRUCTO AI\n"), "{c}");
        for attendu in
            ["mcp__constructo-ai__", "mcp__claude_ai_Constructo_AI__", "ToolSearch", "Se connecter à Constructo AI"]
        {
            assert!(c.contains(attendu), "{attendu} manque :\n{c}");
        }
        assert!(!c.contains("{{") && !c.contains("{qui}"), "{c}");
        assert!(GABARIT_DU_KIT.trim_end().ends_with(&c), "la dernière partie du gabarit");
    }
}
