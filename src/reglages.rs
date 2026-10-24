//! Les réglages d'Eve : `reglages.json`, dans son dossier local ([`dossier_local`], hors du dossier partagé : chaque
//! poste a les siens — son micro, sa voix, qui le tient). Le fichier fait foi : la mémoire d'Eve n'en est qu'une copie,
//! qui se remet à jour quand il change, à la main, par l'installateur ou par l'Assistante elle-même
//! ([`Reglages::relire`]) — jamais l'inverse. Un réglage changé dans la fenêtre s'y inscrit seul, le reste du fichier
//! tel quel ([`Reglages::changer`]) ; un fichier illisible n'est jamais écrasé ; au premier lancement, les valeurs
//! d'office s'y écrivent. Le nom de l'entreprise n'y est pas d'office : Eve le lit au démarrage dans les réglages de
//! l'Adjointe (`config/entreprise.json`, par `scripts/adjointe_config.py --nom`).

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use eve::garde::Espace;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Les mots que Whisper doit reconnaître sans les écorcher : on les lui donne en amorce. Le nom de l'entreprise s'y
/// ajoute au démarrage ([`Reglages::vocabulaire_complet`]) ; les noms propres de chacune (ses clients, ses
/// fournisseurs, ses chantiers) s'ajoutent à la main, dans `vocabulaire` de `reglages.json`.
pub const VOCABULAIRE: &str = "Entrepreneur Général, Eve, Adjointe, chantier, soumission, facture, devis, courriel, \
     brouillon, fournisseur, client, demande de prix, bon de travail, calendrier, comptabilité, TPS, TVQ.";

/// Jessica (Piper, la voix « upmc », locutrice 0) : retenue à l'écoute parmi Siwis, Jessica, Caroline et Nathalie.
pub const VOIX_PAR_DEFAUT: &str = "piper:fr_FR-upmc-medium#0";

/// Ce qu'Eve ne prend qu'en démarrant : son nom et celui de l'entreprise (le titre de sa fenêtre, l'amorce de
/// l'oreille), le dossier de l'Adjointe, l'oreille et son vocabulaire, le débit et la douceur de la voix, le port de la
/// vitrine, la fenêtre, l'avance des rappels, et les privilèges de Claude — son espace d'actions se fixe au démarrage.
/// Le reste vaut tout de suite (le modèle et l'effort de Claude : entre deux demandes, la conversation reprise ; le
/// préchauffage : à la conversation suivante).
const AU_DEMARRAGE: [&str; 14] = [
    "nom", "entreprise", "adjointe", "oreille", "oreille_carte", "oreille_eveil", "vocabulaire", "debit", "douceur",
    "port", "fenetre", "fenetre_native", "alertes.minutes_avant", "privileges",
];

/// Une vérification de `reglages.json` au plus par seconde ([`Reglages::relire`]).
const RELIRE_APRES: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Reglages {
    /// Le nom d'Eve dans sa fenêtre, quand `EVE_NOM` n'en pose pas (vide : « Adjointe ») : voir [`nom_affiche`].
    pub nom: String,
    /// Qui est devant le poste (son prénom, choisi dans la fenêtre) : l'Adjointe le sait avant la première demande.
    pub qui: String,
    /// L'entreprise, pour l'écran et la consigne ; vide : le nom des réglages de l'Adjointe, lu au démarrage.
    pub entreprise: String,
    /// Le nom lu au démarrage dans les réglages de l'Adjointe, quand `entreprise` est vide : jamais écrit.
    #[serde(skip)]
    pub entreprise_lue: String,
    /// Le dossier `Scan` de l'entreprise, lu au démarrage dans les réglages de l'Adjointe : jamais écrit.
    #[serde(skip)]
    pub scan_lu: Option<PathBuf>,
    /// Le dossier de l'Adjointe (`CLAUDE.md`, `scripts\`, `.mcp.json`), s'il n'est pas celui d'Eve elle-même.
    pub adjointe: String,
    /// Le modèle de Whisper pour les demandes ; vide : `modeles\ggml-small-q5_1.bin` du dossier local.
    pub oreille: String,
    /// Le modèle de Whisper sur la carte graphique ; vide : large-v3-turbo s'il est là
    /// (`modeles\ggml-large-v3-turbo-q5_0.bin`), sinon celui des demandes. Sur la carte, il va aussi vite que small et
    /// entend plus juste (mesuré sur une GTX 1050 Ti : 1,2 s contre 1,4 s pour 10,8 s de parole) ; sur le processeur, il
    /// serait bien trop lent : le repli garde `oreille`.
    pub oreille_carte: String,
    /// Le mot d'éveil : Eve écoute en continu et répond quand une phrase commence par « Eve ».
    pub eveil: bool,
    /// Le modèle, plus rapide, qui cherche « Eve » en tête de chaque phrase ; vide : `modeles\ggml-base-q5_1.bin`.
    pub oreille_eveil: String,
    /// L'amorce de Whisper (les noms propres de l'entreprise : ses clients, ses fournisseurs, ses chantiers).
    pub vocabulaire: String,
    /// La voix (« piper:fr_FR-upmc-medium#0 », « windows:Microsoft Caroline ») ; par défaut Jessica, retenue à
    /// l'écoute des quatre voix ; introuvable : la première voix de femme en français.
    pub voix: String,
    /// Le débit de la voix (1 : normal ; 0,85 : celui de Jessica, retenu à l'écoute).
    pub debit: f64,
    /// La douceur de la voix neuronale : 0 telle quelle, 1 douce (le réglage A : une voix plus calme, des pauses un
    /// peu plus longues, les aigus adoucis), 2 plus douce encore (le réglage B).
    pub douceur: u8,
    /// La touche à tenir pour parler : ctrl_droit, ctrl_gauche, alt_droit, f8, f9, pause, arret_defil.
    pub touche: String,
    /// Le modèle de Claude.
    pub modele: String,
    /// L'effort de Claude (low, medium, high, xhigh, max) : plus haut, plus soigné, mais plus lent à répondre.
    pub effort: String,
    /// Le port local de la fenêtre.
    pub port: u16,
    /// Ouvrir la fenêtre au démarrage.
    pub fenetre: bool,
    /// La fenêtre native, dans le programme (le lot 6), au lieu de la page dans Edge. Faux d'office tant qu'elle ne
    /// fait pas tout ce que fait la page ; `eve --native` l'essaie pour un lancement.
    pub fenetre_native: bool,
    /// Le thème de la fenêtre native : « centre » (le centre de commande, au style du tableau HTML), ou « constructo »
    /// (clair et simple, comme l'ERP de Constructo AI). Il se choisit dans ses réglages.
    pub theme: String,
    /// Le dossier de la comptabilité du ménage, celui de son tableau de bord (`tableau\donnees.json`, sa forme dans
    /// `tableau\format-donnees.md`) ; vide : aucune. Nommée, elle seule donne à la fenêtre l'encaisse et les échéances
    /// fiscales, à la place de `activite\budget.js` et des échéances « fiscal » d'`activite\echeances.js`, ses copies
    /// ([`crate::centre`]). Un chemin relatif part du dossier de travail. Il vaut tout de suite.
    pub comptabilite: String,
    /// M'interrompre de la voix : pendant qu'Eve parle, « Eve, stop » (ou « Eve, … ») la coupe. Avec des
    /// haut-parleurs, sa propre voix passe aussi à l'oreille, qui l'ignore : elle ne commence pas par « Eve ».
    pub interruption: bool,
    /// La sourdine : Eve ne parle plus à voix haute, elle répond par écrit dans sa fenêtre. Elle écoute toujours.
    pub sourdine: bool,
    /// Les alertes vocales.
    pub alertes: Alertes,
    /// Toujours devant les autres fenêtres.
    pub devant: bool,
    /// Reprendre les derniers échanges au démarrage (la mémoire d'un jour à l'autre, propre à chaque personne).
    pub memoire: bool,
    /// Garder l'historique des conversations (sur le poste, par personne : `historique\<Prénom>\`).
    pub historique: bool,
    /// Un premier tour silencieux au démarrage : la première vraie question va plus vite.
    pub prechauffage: bool,
    /// Garder le contexte au chaud : un tour silencieux toutes les cinquante minutes sans demande, aux heures de bureau.
    /// La première question après une pause répond alors en 5 à 6 s au lieu de 13 à 24 s, mais chaque tour se paie :
    /// environ 0,02 $ avec Opus 5.5, le contexte encore en cache ; 0,63 $ pour le premier du matin, le cache refroidi
    /// pendant la nuit (le journal du poste d'architecture, 2026-10-22 et 23 — le « 1,28 $ » noté d'abord était le
    /// cumul de la séance). Faux d'office, comme le pose l'installateur : ces tours se paient sur le compte de la
    /// personne, qui les demande (`"au_chaud": true`, dit le LISEZ-MOI).
    pub au_chaud: bool,
    /// Ce que Claude peut faire pour cette Eve : chaque privilège s'active ici ([`Privileges`]).
    pub privileges: Privileges,
    /// Les profils de cette Eve, en plus de celui d'office (son dossier de travail, nommé par `nom`) : chacun un
    /// dossier qui porte son `CLAUDE.md` ([`Profil`]). `eve --profil-ajouter` les inscrit ([`crate::profils`]).
    pub profils: Vec<Profil>,
    /// Le profil choisi, par son nom ; vide : celui d'office. Eve y passe entre deux demandes.
    pub profil: String,
    /// Ce que cette Eve sait de son fichier : jamais écrit ([`Fichier`]).
    #[serde(skip)]
    fichier: Fichier,
}

/// Les privilèges de Claude (lot 1.1), chacun donné ou retiré ici, à la main. Un privilège absent prend la valeur de la
/// sorte d'Eve ([`Reglages::espace`]) : un spécialiste a d'office la liberté totale (depuis le 24 octobre 2026), et
/// l'Adjointe d'Entrepreneur Général garde ce qu'elle a toujours eu. Un privilège écrit garde sa valeur, quoi que
/// devienne celle d'office. Seuls les privilèges donnés s'écrivent dans le fichier.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Privileges {
    /// Les connecteurs de claude.ai (Constructo AI, Gmail…).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connecteurs: Option<bool>,
    /// Chrome, avec les sessions ouvertes de la personne.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chrome: Option<bool>,
    /// Les serveurs MCP du poste (ceux de l'utilisateur et du projet : FreeCAD…).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mcp_du_poste: Option<bool>,
    /// Constructo AI par la connexion d'Eve elle-même, au compte de la personne (« Se connecter à Constructo AI »,
    /// dans les réglages de sa fenêtre).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub constructo: Option<bool>,
    /// Les accords à la voix : « je confirme » avant une suppression demandée à la voix et toute écriture dans
    /// Constructo AI. Faux : Eve ne demande plus rien, et le journal note ce qui l'aurait demandé.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accords: Option<bool>,
    /// Les outils de Claude Code, par leur nom : « Read », « Write », « Edit », « Glob », « Grep », « WebSearch »,
    /// « WebFetch », « PowerShell », « Bash »… ; « tous » les donne tous. Un nom fait d'autre chose que de lettres, de
    /// chiffres et de soulignés ne compte pas.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outils: Option<Vec<String>>,
}

/// Un profil de cette Eve : un dossier de travail qui porte son `CLAUDE.md` (d'ordinaire un `PROFIL.md` qu'il importe,
/// peut-être sa consigne vocale, `EVE_CONSIGNE.md`) — la forme du dossier d'un spécialiste du kit.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Profil {
    /// Son nom, qui le désigne sans égard à la casse : celui du menu de la fenêtre, et celui que dit la personne.
    pub nom: String,
    /// Son dossier, un chemin complet : Claude Code y travaille, avec son `CLAUDE.md`.
    pub dossier: PathBuf,
}

/// Les alertes vocales : ce qu'Eve dit d'elle-même, une cloche d'abord.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Alertes {
    /// Un courriel suspect arrive (règle 6).
    pub suspects: bool,
    /// Une facture arrive (pièce d'un courriel ou numérisation dont le nom dit « facture »).
    pub factures: bool,
    /// Les autres pièces et numérisations à classer.
    pub pieces: bool,
    /// Un rendez-vous du calendrier approche.
    pub rendez_vous: bool,
    /// Combien de minutes avant un rendez-vous.
    pub minutes_avant: u32,
}

/// `reglages.json` comme cette Eve le connaît : où il est ; la version que la mémoire en a prise, et celle qu'a vue la
/// dernière vérification (leur date et leur taille) ; quand il a été vérifié ; et pourquoi il ne se lit pas, s'il ne se
/// lit pas — tant qu'il ne se lit pas, Eve n'y écrit rien.
#[derive(Debug, Clone, Default)]
struct Fichier {
    chemin: PathBuf,
    lue: Date,
    vue: Date,
    verifie: Option<Instant>,
    illisible: Option<String>,
}

/// La date d'écriture d'un fichier et sa taille ; `None` : il n'existe pas. La fenêtre suit de même le tableau de la
/// comptabilité ([`crate::centre`]).
pub(crate) type Date = Option<(SystemTime, u64)>;

pub(crate) fn date_de(chemin: &Path) -> Date {
    let m = fs::metadata(chemin).ok()?;
    Some((m.modified().unwrap_or(SystemTime::UNIX_EPOCH), m.len()))
}

impl Default for Alertes {
    fn default() -> Self {
        Alertes { suspects: true, factures: true, pieces: false, rendez_vous: true, minutes_avant: 15 }
    }
}

impl Default for Reglages {
    fn default() -> Self {
        Reglages {
            nom: String::new(),
            // Personne d'office : le prénom se donne dans les réglages de la fenêtre, ou l'Adjointe le demande.
            qui: String::new(),
            entreprise: String::new(),
            entreprise_lue: String::new(),
            scan_lu: None,
            adjointe: String::new(),
            oreille: String::new(),
            oreille_carte: String::new(),
            eveil: true,
            oreille_eveil: String::new(),
            vocabulaire: VOCABULAIRE.into(),
            voix: VOIX_PAR_DEFAUT.into(),
            debit: 0.85,
            douceur: 1,
            touche: "ctrl_droit".into(),
            modele: "claude-sonnet-5-5".into(),
            // « medium » : au plus haut, une question simple a pris sept minutes ; la voix demande vite.
            effort: "medium".into(),
            port: 47811,
            fenetre: true,
            fenetre_native: false,
            theme: "centre".into(),
            comptabilite: String::new(),
            interruption: true,
            sourdine: false,
            alertes: Alertes::default(),
            devant: false,
            memoire: true,
            historique: true,
            prechauffage: true,
            au_chaud: false,
            privileges: Privileges::default(),
            profils: Vec::new(),
            profil: String::new(),
            fichier: Fichier::default(),
        }
    }
}

/// Une Eve est un spécialiste quand son dossier de travail porte sa consigne (`EVE_CONSIGNE.md`, même vide ou
/// illisible), ou que son dossier local est celui d'un spécialiste du kit (`…\Eve\specialistes\<id>`) : un fichier
/// vidé ou retiré ne lui ouvre jamais l'espace de l'Adjointe.
pub fn est_un_specialiste(dossier_de_travail: &Path) -> bool {
    dossier_de_travail.join("EVE_CONSIGNE.md").exists()
        || dossier_local().parent().and_then(Path::file_name).is_some_and(|n| n.eq_ignore_ascii_case("specialistes"))
}

/// Le dossier local d'Eve, hors du dossier partagé : réglages, journal, modèles, voix, pièces jointes. `EVE_DOSSIER_LOCAL`
/// en nomme un autre : une seconde Eve sur le même poste (celle d'un poste d'architecture) garde ses réglages, sa mémoire
/// et son historique à part.
pub fn dossier_local() -> PathBuf {
    let d = match std::env::var_os("EVE_DOSSIER_LOCAL").filter(|v| !v.is_empty()) {
        Some(d) => PathBuf::from(d),
        None => {
            let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
            base.join("EntrepreneurGeneral").join("Eve")
        }
    };
    let _ = fs::create_dir_all(&d);
    d
}

/// Écrit un fichier d'un seul geste : le contenu va dans un fichier à côté (`<nom>.tmp`), poussé jusqu'au disque, qui
/// prend ensuite la place de l'ancien par un renommage — que Windows fait d'un coup. Une coupure en route laisse
/// l'ancien fichier entier (au pire, un `<nom>.tmp` à côté, que l'écriture suivante remplace) ; un fichier qu'un autre
/// programme tient ouvert sans partage n'est pas remplacé : une erreur, et l'ancien intact. La seule façon dont Eve
/// réécrit un fichier en entier : ses réglages, sa mémoire, sa session, sa connexion à Constructo AI.
pub fn ecrire_d_un_geste(chemin: &Path, contenu: &[u8]) -> io::Result<()> {
    let mut nom = chemin.file_name().ok_or_else(|| io::Error::other("un chemin sans nom de fichier"))?.to_os_string();
    nom.push(".tmp");
    let a_cote = chemin.with_file_name(nom);
    let ecrit = File::create(&a_cote)
        .and_then(|mut f| {
            f.write_all(contenu)?;
            f.sync_all()
        })
        .and_then(|()| fs::rename(&a_cote, chemin));
    if ecrit.is_err() {
        let _ = fs::remove_file(&a_cote);
    }
    ecrit
}

/// Le nom affiché sous « Eve » et dans le titre de sa fenêtre : celui que nomme `EVE_NOM` (posé par le lanceur d'un poste :
/// « Designer » sur le poste d'architecture), sinon `nom` de `reglages.json` (un nom accentué ne passe pas par un
/// `.bat`), sinon « Adjointe ». Lu une fois.
pub fn nom_affiche() -> &'static str {
    static NOM: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    NOM.get_or_init(|| {
        let pose = std::env::var("EVE_NOM").ok().filter(|n| !n.trim().is_empty());
        nom_valable(pose.or_else(|| lire(&fichier()).ok().flatten().map(|(_, r)| r.nom)))
    })
}

fn nom_valable(nom: Option<String>) -> String {
    nom.map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty() && n.chars().count() <= 40 && !n.chars().any(char::is_control))
        .unwrap_or_else(|| "Adjointe".into())
}

/// Le dossier d'Eve lui-même (la trousse : `Eve.bat`, `bin\`, `scripts\`) : celui que nomme `EVE_DOSSIER` (posé par
/// `Eve.bat`, qui peut lancer une copie locale de l'exécutable : celle d'un dossier synchronisé n'est alors jamais
/// verrouillée), sinon celui de `Eve.bat`, au-dessus de l'exécutable (`bin\eve.exe` dans la trousse,
/// `target\release\eve.exe` à la construction).
pub fn dossier_d_eve() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("EVE_DOSSIER").map(PathBuf::from).filter(|d| d.join("Eve.bat").is_file()) {
        return Some(d);
    }
    let exe = std::env::current_exe().ok()?;
    exe.ancestors().skip(1).take(4).find(|d| d.join("Eve.bat").is_file()).map(Path::to_path_buf)
}

/// Une ressource d'Eve (`piper`, `voix`, `modeles`, `whisper-gpu`) : celle du dossier local du poste (où `Eve.bat` la
/// télécharge au premier lancement), sinon celle de `ressources\` du dossier d'Eve. `whisper-gpu` est facultative :
/// sans elle, l'oreille tourne sur le processeur.
///
/// Le local d'abord : Piper et le serveur de Whisper pour la carte graphique n'ouvrent pas un chemin accentué (dans un
/// dossier « Général », mesuré : Piper ne démarrait pas, le serveur plantait sur « G?n?ral »). Le profil local
/// (`C:\Users\<session>\AppData\Local`) n'en a pas, sauf un nom de session accentué.
pub fn ressource(nom: &str) -> PathBuf {
    let locale = dossier_local().join(nom);
    if locale.exists() {
        return locale;
    }
    if let Some(d) = dossier_d_eve() {
        let p = d.join("ressources").join(nom);
        if p.exists() {
            return p;
        }
    }
    locale
}

fn fichier() -> PathBuf {
    dossier_local().join("reglages.json")
}

/// Les réglages du poste ([`charger_de`]).
pub fn charger() -> Reglages {
    charger_de(fichier())
}

/// Les réglages du poste tels que le fichier les dit, sans rien y écrire (absent : les valeurs d'office) ; s'il ne se
/// lit pas, pourquoi : pour une commande qui ne fait que lire (`eve --profils`).
pub fn lire_du_poste() -> Result<Reglages, String> {
    let chemin = fichier();
    let mut r = lire(&chemin)
        .map_err(|raison| format!("{} ne se lit pas ({raison})", chemin.display()))?
        .map(|(_, r)| r)
        .unwrap_or_default();
    r.fichier.chemin = chemin;
    Ok(r)
}

/// Les réglages du poste, pour une commande qui y écrit (`eve --profil`, `eve --modele`) : un fichier qui ne se lit
/// pas n'est jamais touché.
pub fn a_changer() -> Result<Reglages, String> {
    let r = charger();
    match r.illisible() {
        Some(raison) => Err(format!("{} ne se lit pas ({raison}) : rien n'a changé.", r.chemin().display())),
        None => Ok(r),
    }
}

/// La fin d'une commande : ce qu'elle a fait, en une ligne, ou pourquoi elle refuse (sur la sortie des erreurs) ; rend
/// son code de sortie (0 : fait).
pub fn sortie_de_commande(fait: Result<String, String>) -> i32 {
    match fait {
        Ok(m) => {
            println!("{m}");
            0
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

// ---- Le cerveau : le modèle et l'effort de Claude -----------------------------------------------------------------

/// Les modèles de Claude qu'Eve offre (la rubrique « Cerveau » de ses réglages, `eve --modele`) : l'identifiant que
/// prend Claude Code (`--model`), son nom, et le mot qui le désigne (celui que dit la personne, et le paramètre `model`
/// de l'outil Agent). Sonnet d'office : il répond vite, ce qu'il faut à une conversation à voix haute ; Opus réfléchit
/// plus loin, plus lentement et plus cher.
pub const MODELES: [(&str, &str, &str); 2] =
    [("claude-sonnet-5-5", "Sonnet 5.5", "sonnet"), ("claude-opus-5-5", "Opus 5.5", "opus")];

/// Les efforts qu'Eve offre : celui que prend Claude Code (`--effort`, et le paramètre `effort` de l'outil Agent), et
/// son nom.
pub const EFFORTS: [(&str, &str); 2] = [("medium", "moyen"), ("max", "max")];

/// Comment s'écrivent les commandes du cerveau, quand elles sont mal écrites.
const USAGE_DU_CERVEAU: &str = "eve --modele [sonnet|opus [moyen|max]] | --effort moyen|max";

/// Le nom d'un modèle (« Opus 5.5 ») ; un modèle qu'Eve n'offre pas garde son identifiant.
pub fn nom_du_modele(id: &str) -> &str {
    MODELES.iter().find(|m| m.0 == id).map_or(id, |m| m.1)
}

/// Le nom d'un effort (« moyen ») ; un effort qu'Eve n'offre pas garde le sien.
pub fn nom_de_l_effort(id: &str) -> &str {
    EFFORTS.iter().find(|e| e.0 == id).map_or(id, |e| e.1)
}

/// Le modèle que désigne `mot` : son identifiant, son nom ou son mot (« opus », « Opus 5.5 »), sans égard à la casse.
pub fn modele_dit(mot: &str) -> Option<&'static str> {
    let mot = mot.trim().to_lowercase();
    MODELES.iter().find(|m| [m.0, m.1, m.2].iter().any(|x| x.to_lowercase() == mot)).map(|m| m.0)
}

/// L'effort que désigne `mot` (« max », « medium », « moyen »), sans égard à la casse.
pub fn effort_dit(mot: &str) -> Option<&'static str> {
    let mot = mot.trim().to_lowercase();
    EFFORTS.iter().find(|e| e.0 == mot || e.1 == mot).map(|e| e.0)
}

/// Les commandes du cerveau, si `args` en est une : `eve --modele` (le modèle et l'effort de cette Eve, et les choix),
/// `eve --modele <modèle> [<effort>]`, `eve --effort <effort>`. Elles écrivent dans `reglages.json` du dossier local
/// (`EVE_DOSSIER_LOCAL`) comme la fenêtre ([`Reglages::changer`]) ; l'Eve en marche relit le fichier et relance Claude
/// Code avec eux entre deux demandes, la conversation reprise. Ni le journal ni la vitrine ne s'ouvrent.
pub fn commande(args: &[String]) -> Option<i32> {
    let (premier, reste) = args.split_first()?;
    let mots: Vec<&str> = reste.iter().map(String::as_str).collect();
    let fait = match (premier.as_str(), mots.as_slice()) {
        ("--modele", &[]) => lire_du_poste().map(|r| cerveau_de(&r)),
        ("--modele", &[modele]) => a_changer().and_then(|mut r| changer_le_cerveau(&mut r, Some(modele), None)),
        ("--modele", &[modele, effort]) => {
            a_changer().and_then(|mut r| changer_le_cerveau(&mut r, Some(modele), Some(effort)))
        }
        ("--effort", &[effort]) => a_changer().and_then(|mut r| changer_le_cerveau(&mut r, None, Some(effort))),
        ("--modele" | "--effort", _) => Err(USAGE_DU_CERVEAU.to_string()),
        _ => return None,
    };
    Some(sortie_de_commande(fait))
}

/// Le cerveau de cette Eve, et les choix : « Sonnet 5.5, effort moyen ; les choix : Sonnet 5.5, Opus 5.5 ; effort
/// moyen ou max. »
fn cerveau_de(r: &Reglages) -> String {
    format!(
        "Le cerveau de cette Eve (dans {}) : {}, effort {} ; les choix : {} ; effort {}.",
        r.chemin().display(),
        nom_du_modele(&r.modele),
        nom_de_l_effort(&r.effort),
        MODELES.iter().map(|m| m.1).collect::<Vec<_>>().join(", "),
        EFFORTS.iter().map(|e| e.1).collect::<Vec<_>>().join(" ou ")
    )
}

/// Le modèle et l'effort de Claude changés dans `r` (l'un, l'autre ou les deux), parmi ceux qu'Eve offre : un mot
/// qu'elle ne connaît pas est refusé, et rien ne change.
pub fn changer_le_cerveau(r: &mut Reglages, modele: Option<&str>, effort: Option<&str>) -> Result<String, String> {
    let modele = modele
        .map(|m| {
            modele_dit(m).ok_or_else(|| {
                let choix = MODELES.iter().map(|x| x.1).collect::<Vec<_>>().join(", ");
                format!("« {} » n'est pas un modèle d'Eve ; les choix : {choix}.", m.trim())
            })
        })
        .transpose()?;
    let effort = effort
        .map(|e| {
            effort_dit(e).ok_or_else(|| {
                let choix = EFFORTS.iter().map(|x| x.1).collect::<Vec<_>>().join(", ");
                format!("« {} » n'est pas un effort d'Eve ; les choix : {choix}.", e.trim())
            })
        })
        .transpose()?;
    if let Some(m) = modele {
        r.changer("modele", Value::from(m))?;
    }
    if let Some(e) = effort {
        r.changer("effort", Value::from(e))?;
    }
    Ok(format!(
        "Le cerveau de cette Eve : {}, effort {} ; elle repart avec lui entre deux demandes, la conversation reprise.",
        nom_du_modele(&r.modele),
        nom_de_l_effort(&r.effort)
    ))
}

/// Les réglages que dit le fichier `chemin` ; absent (le premier lancement), les valeurs d'office, qui s'y écrivent ;
/// illisible, les valeurs d'office en mémoire seulement — le fichier n'est pas touché, et la raison est gardée pour la
/// personne ([`Reglages::illisible`]).
pub(crate) fn charger_de(chemin: PathBuf) -> Reglages {
    let mut r = match lire(&chemin) {
        Ok(Some((_, r))) => r,
        Ok(None) => {
            let _ = ecrire_d_un_geste(&chemin, texte_d_office().as_bytes());
            Reglages::default()
        }
        Err(raison) => {
            let mut r = Reglages::default();
            r.fichier.illisible = Some(raison);
            r
        }
    };
    let date = date_de(&chemin);
    (r.fichier.lue, r.fichier.vue) = (date, date);
    r.fichier.chemin = chemin;
    r
}

/// Ce que dit le fichier : son texte et ses réglages ; `None` s'il n'existe pas ; s'il ne se lit pas, pourquoi (il ne
/// s'ouvre pas, il n'est pas en UTF-8, ce n'est pas du JSON, une valeur n'a pas le bon type).
fn lire(chemin: &Path) -> Result<Option<(String, Reglages)>, String> {
    let octets = match fs::read(chemin) {
        Ok(o) => o,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("il ne s'ouvre pas : {e}")),
    };
    let texte = String::from_utf8(octets).map_err(|_| "il n'est pas écrit en UTF-8".to_string())?;
    let r = analyser(&texte)?;
    Ok(Some((texte, r)))
}

/// Un texte de `reglages.json`, lu comme Eve le lit (une marque d'ordre des octets en tête ne compte pas).
fn analyser(texte: &str) -> Result<Reglages, String> {
    serde_json::from_str(texte.trim_start_matches('\u{feff}')).map_err(|e| e.to_string())
}

/// Le fichier du premier lancement : toutes les valeurs d'office, pour qu'on les voie et les corrige.
fn texte_d_office() -> String {
    serde_json::to_string_pretty(&Reglages::default()).unwrap_or_default()
}

/// La valeur d'un réglage dans `r`, par son pointeur JSON (« /alertes/factures ») ; `None` : ce réglage n'existe pas.
fn valeur_de(r: &Reglages, pointeur: &str) -> Option<Value> {
    serde_json::to_value(r).ok()?.pointer(pointeur).cloned()
}

/// Les réglages qui diffèrent entre `avant` et `apres`, chacun par son chemin (« sourdine », « alertes.factures »,
/// « privileges.outils »).
pub fn changements(avant: &Reglages, apres: &Reglages) -> Vec<String> {
    let valeur = |r: &Reglages| serde_json::to_value(r).unwrap_or_default();
    let mut cles = Vec::new();
    ecarts(&valeur(avant), &valeur(apres), "", &mut cles);
    cles
}

fn ecarts(a: &Value, b: &Value, chemin: &str, cles: &mut Vec<String>) {
    if let (Value::Object(x), Value::Object(y)) = (a, b) {
        for k in x.keys().chain(y.keys().filter(|k| !x.contains_key(*k))) {
            let sous = if chemin.is_empty() { k.clone() } else { format!("{chemin}.{k}") };
            ecarts(x.get(k).unwrap_or(&Value::Null), y.get(k).unwrap_or(&Value::Null), &sous, cles);
        }
    } else if a != b {
        cles.push(chemin.to_string());
    }
}

/// Ce réglage (« privileges.chrome ») n'agit-il qu'au prochain démarrage d'Eve ([`AU_DEMARRAGE`]) ?
pub fn au_prochain_demarrage(cle: &str) -> bool {
    AU_DEMARRAGE.iter().any(|p| cle.strip_prefix(p).is_some_and(|reste| reste.is_empty() || reste.starts_with('.')))
}

impl Reglages {
    /// Pourquoi `reglages.json` ne se lit pas ; `None` : il se lit.
    pub fn illisible(&self) -> Option<&str> {
        self.fichier.illisible.as_deref()
    }

    /// Où est `reglages.json`.
    pub fn chemin(&self) -> &Path {
        &self.fichier.chemin
    }

    /// La personne change un réglage dans la fenêtre (`cle` : « sourdine », « alertes.factures »…) : il s'inscrit dans
    /// `reglages.json` relu à l'instant, et rien d'autre n'y change — ni ce qu'on y a écrit d'autre pendant qu'Eve
    /// tournait, ni les champs qu'y a mis une version plus récente d'Eve, ni leur ordre, ni la mise en forme ; puis la
    /// mémoire reprend le fichier tout entier. Une valeur que le fichier a déjà ne s'écrit pas (une valeur d'office n'y
    /// est donc pas figée). Si le fichier ne se lit pas ou ne s'écrit pas, rien ne change : la raison, pour la personne.
    pub fn changer(&mut self, cle: &str, valeur: Value) -> Result<(), String> {
        let pointeur = format!("/{}", cle.replace('.', "/"));
        // Seul un réglage que ce programme connaît s'écrit d'ici : jamais un privilège, qui ne se donne qu'à la main.
        if valeur_de(&Reglages::default(), &pointeur).is_none() {
            return Err(format!("Le réglage « {cle} » n'existe pas."));
        }
        let pas_change = |raison: String| format!("Ce réglage n'a pas changé : {raison}.");
        let (texte, mut actuel) = match lire(&self.fichier.chemin) {
            Ok(Some(lu)) => lu,
            // Absent : le fichier du premier lancement, et ce réglage.
            Ok(None) => (texte_d_office(), Reglages::default()),
            Err(raison) => return Err(pas_change(format!("reglages.json ne se lit pas ({raison})"))),
        };
        if valeur_de(&actuel, &pointeur).as_ref() != Some(&valeur) {
            let neuf = inscrire(&texte, cle, &valeur)
                .ok_or_else(|| pas_change("reglages.json n'a pas la forme d'un objet JSON".into()))?;
            // Relu comme il le sera : une valeur du mauvais type ne s'écrit pas.
            actuel = analyser(&neuf).map_err(pas_change)?;
            ecrire_d_un_geste(&self.fichier.chemin, neuf.as_bytes())
                .map_err(|e| pas_change(format!("reglages.json ne s'écrit pas ({e})")))?;
        }
        self.reprendre(actuel);
        let date = date_de(&self.fichier.chemin);
        (self.fichier.lue, self.fichier.vue) = (date, date);
        Ok(())
    }

    /// Relit `reglages.json` s'il a changé depuis la dernière lecture — sa date ou sa taille, vues une fois par seconde
    /// au plus — et la mémoire le reprend tout entier (absent : les valeurs d'office). Une version neuve ne se lit
    /// qu'une fois restée la même d'une vérification à l'autre : un fichier qu'on est en train d'écrire ne passe pas
    /// pour illisible. Rend les réglages d'avant quand quelque chose a changé : la mémoire, ou le fichier qui cesse de
    /// se lire (la mémoire garde alors ce qu'elle avait) ou qui se lit de nouveau.
    pub fn relire(&mut self) -> Option<Reglages> {
        let f = &mut self.fichier;
        if f.verifie.is_some_and(|t| t.elapsed() < RELIRE_APRES) {
            return None;
        }
        f.verifie = Some(Instant::now());
        let date = date_de(&f.chemin);
        let stable = date == f.vue;
        f.vue = date;
        if date == f.lue || !stable {
            return None;
        }
        f.lue = date;
        let lu = lire(&f.chemin);
        let avant = self.clone();
        match lu {
            Ok(lu) => self.reprendre(lu.map(|(_, r)| r).unwrap_or_default()),
            Err(raison) if self.fichier.illisible.is_none() => self.fichier.illisible = Some(raison),
            Err(_) => return None,
        }
        let lisible_change = avant.fichier.illisible.is_some() != self.fichier.illisible.is_some();
        (lisible_change || !changements(&avant, self).is_empty()).then_some(avant)
    }

    /// La mémoire reprend `neuf`, ce que dit le fichier ; ce qui ne vient pas de lui reste : le nom de l'entreprise et
    /// le dossier `Scan` lus au démarrage, et ce qu'Eve sait du fichier même — qui se lit, puisque `neuf` en vient.
    fn reprendre(&mut self, neuf: Reglages) {
        let entreprise_lue = std::mem::take(&mut self.entreprise_lue);
        let scan_lu = self.scan_lu.take();
        let mut fichier = std::mem::take(&mut self.fichier);
        fichier.illisible = None;
        *self = Reglages { entreprise_lue, scan_lu, fichier, ..neuf };
    }

    /// Le dossier de l'Adjointe : celui d'Eve elle-même (la trousse : `Eve.bat` à côté de `scripts\` et du `CLAUDE.md`
    /// que l'installation y crée), sinon celui du réglage `adjointe` — le premier qui porte un `CLAUDE.md` et un dossier
    /// `scripts`. Le chemin compte : Claude Code range sa mémoire et l'accord donné au serveur MCP par chemin de projet.
    /// Cette Eve est-elle un spécialiste ? Oui dans le dossier d'un spécialiste (`est_un_specialiste`), et sans
    /// dossier de travail : elle n'est l'Adjointe d'Entrepreneur Général que dans le sien.
    pub fn specialiste(&self) -> bool {
        self.dossier_adjointe().as_deref().is_none_or(est_un_specialiste)
    }

    pub fn dossier_adjointe(&self) -> Option<PathBuf> {
        let mut candidats: Vec<PathBuf> = Vec::new();
        candidats.extend(dossier_d_eve());
        if !self.adjointe.is_empty() {
            candidats.push(PathBuf::from(&self.adjointe));
        }
        candidats.into_iter().find(|d| d.join("CLAUDE.md").is_file() && d.join("scripts").is_dir())
    }

    /// Le nom de l'entreprise : celui du réglage, sinon celui des réglages de l'Adjointe ; « » s'il n'y en a aucun.
    pub fn nom_entreprise(&self) -> &str {
        let propre = self.entreprise.trim();
        if propre.is_empty() { self.entreprise_lue.trim() } else { propre }
    }

    /// L'amorce de Whisper : le nom de l'entreprise en tête (s'il n'y est pas déjà), puis le vocabulaire.
    pub fn vocabulaire_complet(&self) -> String {
        let nom = self.nom_entreprise();
        if nom.is_empty() || self.vocabulaire.contains(nom) {
            self.vocabulaire.clone()
        } else {
            format!("{nom}, {}", self.vocabulaire)
        }
    }

    pub fn chemin_oreille(&self) -> PathBuf {
        if self.oreille.is_empty() {
            ressource("modeles").join("ggml-small-q5_1.bin")
        } else {
            PathBuf::from(&self.oreille)
        }
    }

    /// Le modèle de la carte graphique (voir `oreille_carte`).
    pub fn chemin_oreille_carte(&self) -> PathBuf {
        if !self.oreille_carte.is_empty() {
            return PathBuf::from(&self.oreille_carte);
        }
        let turbo = ressource("modeles").join("ggml-large-v3-turbo-q5_0.bin");
        if turbo.is_file() { turbo } else { self.chemin_oreille() }
    }

    pub fn chemin_oreille_eveil(&self) -> PathBuf {
        if self.oreille_eveil.is_empty() {
            ressource("modeles").join("ggml-base-q5_1.bin")
        } else {
            PathBuf::from(&self.oreille_eveil)
        }
    }

    /// L'espace d'actions de Claude : les privilèges de `reglages.json`, et pour ce qui n'y est pas, ceux de la sorte
    /// d'Eve ([`est_un_specialiste`]).
    pub fn espace(&self, specialiste: bool) -> Espace {
        let sorte = if specialiste { Espace::specialiste() } else { Espace::adjointe() };
        let p = &self.privileges;
        let outils = match &p.outils {
            Some(o) if o.iter().any(|x| x.trim().eq_ignore_ascii_case("tous")) => None,
            Some(o) => Some(
                o.iter()
                    .map(|x| x.trim().to_string())
                    .filter(|x| !x.is_empty() && x.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
                    .collect(),
            ),
            None => sorte.outils,
        };
        Espace {
            connecteurs: p.connecteurs.unwrap_or(sorte.connecteurs),
            chrome: p.chrome.unwrap_or(sorte.chrome),
            mcp_du_poste: p.mcp_du_poste.unwrap_or(sorte.mcp_du_poste),
            constructo: p.constructo.unwrap_or(sorte.constructo),
            accords: p.accords.unwrap_or(sorte.accords),
            outils,
        }
    }

    /// La touche à tenir : son code de touche virtuelle Windows et son nom à l'écran.
    pub fn touche(&self) -> (i32, &'static str) {
        match self.touche.as_str() {
            "ctrl_gauche" => (0xA2, "Ctrl gauche"),
            "alt_droit" => (0xA5, "Alt droit"),
            "f8" => (0x77, "F8"),
            "f9" => (0x78, "F9"),
            "pause" => (0x13, "Pause"),
            "arret_defil" => (0x91, "Arrêt défil"),
            _ => (0xA3, "Ctrl droit"),
        }
    }
}

// ---- Un réglage inscrit dans le texte du fichier ------------------------------------------------------------------

/// Un membre d'un objet JSON : son nom, et où commencent et finissent son nom (ses guillemets) et sa valeur.
struct Membre {
    nom: String,
    debut_nom: usize,
    fin_nom: usize,
    debut: usize,
    fin: usize,
}

/// `texte` (un objet JSON) où la valeur de `cle` (« sourdine », « alertes.factures ») devient `valeur`, et rien d'autre
/// ne change : ni les autres champs, connus d'Eve ou non, ni leur ordre, ni les blancs, les fins de ligne ou la marque
/// d'ordre des octets. Une clé absente s'ajoute au bout de son objet, comme ses voisines (et l'objet aussi, s'il
/// manque). `None` : ce n'est pas un objet, ou le chemin passe par une valeur qui n'en est pas un.
fn inscrire(texte: &str, cle: &str, valeur: &Value) -> Option<String> {
    let corps = texte.trim_start_matches('\u{feff}');
    let marque = &texte[..texte.len() - corps.len()];
    let t = corps.as_bytes();
    let noms: Vec<&str> = cle.split('.').collect();
    let mut objet = blancs(t, 0);
    for (n, nom) in noms.iter().enumerate() {
        if t.get(objet) != Some(&b'{') {
            return None;
        }
        let (membres, fin) = membres(t, objet)?;
        let Some(m) = membres.iter().find(|m| m.nom == *nom) else {
            // Ce qui manque du chemin vient avec lui : « alertes » absent, `"alertes": {"factures": …}`.
            let v = noms[n + 1..]
                .iter()
                .rev()
                .fold(valeur.clone(), |v, nom| Value::Object([(nom.to_string(), v)].into_iter().collect()));
            return Some(format!("{marque}{}", ajouter(corps, &membres, objet, fin, nom, &v)));
        };
        if n + 1 == noms.len() {
            let rendu = rendu(valeur, blanc_devant(corps, m.debut_nom), fin_de_ligne(corps));
            return Some(format!("{marque}{}{rendu}{}", &corps[..m.debut], &corps[m.fin..]));
        }
        objet = m.debut;
    }
    None
}

/// `"nom": valeur` ajouté au bout de l'objet qui va de `debut` (son `{`) à `fin` (son `}`) : après son dernier membre,
/// avec le même blanc devant et les mêmes deux-points ; dans un objet vide, sur sa ligne, deux espaces plus loin.
fn ajouter(corps: &str, membres: &[Membre], debut: usize, fin: usize, nom: &str, valeur: &Value) -> String {
    let nl = fin_de_ligne(corps);
    let nom = Value::from(nom).to_string();
    match membres.last() {
        Some(d) => {
            let (blanc, deux_points) = (blanc_devant(corps, d.debut_nom), &corps[d.fin_nom..d.debut]);
            let rendu = rendu(valeur, blanc, nl);
            format!("{},{blanc}{nom}{deux_points}{rendu}{}", &corps[..d.fin], &corps[d.fin..])
        }
        None => {
            let ligne = &corps[corps[..debut].rfind('\n').map_or(0, |i| i + 1)..debut];
            let retrait: String = ligne.chars().take_while(|c| matches!(c, ' ' | '\t')).collect();
            let blanc = format!("{nl}{retrait}  ");
            let rendu = rendu(valeur, &blanc, nl);
            format!("{}{{{blanc}{nom}: {rendu}{nl}{retrait}}}{}", &corps[..debut], &corps[fin + 1..])
        }
    }
}

/// Le blanc qui précède le nom d'un membre, depuis la virgule ou l'accolade d'avant (« \n  », une espace, rien).
fn blanc_devant(corps: &str, debut_nom: usize) -> &str {
    let avant = &corps[..debut_nom];
    &avant[avant.trim_end().len()..]
}

/// Les fins de ligne du fichier : celles de Windows s'il en a.
fn fin_de_ligne(corps: &str) -> &'static str {
    if corps.contains("\r\n") { "\r\n" } else { "\n" }
}

/// Une valeur écrite à sa place : sur une ligne ; un objet ou un tableau sous un membre qui a sa propre ligne (`blanc`,
/// le blanc devant le membre) s'étale sur les suivantes, sous lui.
fn rendu(valeur: &Value, blanc: &str, nl: &str) -> String {
    match blanc.rfind('\n') {
        Some(i) if valeur.is_object() || valeur.is_array() => {
            serde_json::to_string_pretty(valeur).unwrap_or_default().replace('\n', &format!("{nl}{}", &blanc[i + 1..]))
        }
        _ => valeur.to_string(),
    }
}

/// Les membres de l'objet dont l'accolade ouvrante est à `debut`, et la place de son accolade fermante.
fn membres(t: &[u8], debut: usize) -> Option<(Vec<Membre>, usize)> {
    let mut liste = Vec::new();
    let mut i = blancs(t, debut + 1);
    if t.get(i) == Some(&b'}') {
        return Some((liste, i));
    }
    loop {
        let fin_nom = fin_de_chaine(t, i)?;
        let nom = serde_json::from_slice(&t[i..fin_nom]).ok()?;
        let deux_points = blancs(t, fin_nom);
        if t.get(deux_points) != Some(&b':') {
            return None;
        }
        let debut_valeur = blancs(t, deux_points + 1);
        let fin = fin_de_valeur(t, debut_valeur)?;
        liste.push(Membre { nom, debut_nom: i, fin_nom, debut: debut_valeur, fin });
        i = blancs(t, fin);
        match t.get(i)? {
            b',' => i = blancs(t, i + 1),
            b'}' => return Some((liste, i)),
            _ => return None,
        }
    }
}

/// Le premier caractère qui n'est pas un blanc de JSON, à partir de `i`.
fn blancs(t: &[u8], mut i: usize) -> usize {
    while t.get(i).is_some_and(|c| matches!(c, b' ' | b'\t' | b'\n' | b'\r')) {
        i += 1;
    }
    i
}

/// La fin (après le guillemet fermant) de la chaîne qui commence à `i` : une barre oblique inverse protège le caractère
/// qui la suit.
fn fin_de_chaine(t: &[u8], i: usize) -> Option<usize> {
    if t.get(i) != Some(&b'"') {
        return None;
    }
    let mut j = i + 1;
    loop {
        match t.get(j)? {
            b'\\' => j += 2,
            b'"' => return Some(j + 1),
            _ => j += 1,
        }
    }
}

/// La fin de la valeur qui commence à `i` : une chaîne ; un objet ou un tableau, jusqu'à ce qui le referme (une
/// accolade dans une chaîne ne compte pas) ; un nombre ou un mot (`true`, `false`, `null`).
fn fin_de_valeur(t: &[u8], i: usize) -> Option<usize> {
    match t.get(i)? {
        b'"' => fin_de_chaine(t, i),
        b'{' | b'[' => {
            let (mut j, mut profondeur) = (i, 0usize);
            loop {
                match t.get(j)? {
                    b'"' => {
                        j = fin_de_chaine(t, j)?;
                        continue;
                    }
                    b'{' | b'[' => profondeur += 1,
                    b'}' | b']' => {
                        profondeur -= 1;
                        if profondeur == 0 {
                            return Some(j + 1);
                        }
                    }
                    _ => {}
                }
                j += 1;
            }
        }
        _ => {
            let fin = t[i..]
                .iter()
                .position(|c| matches!(c, b',' | b'}' | b']' | b' ' | b'\t' | b'\n' | b'\r'))
                .map_or(t.len(), |n| i + n);
            (fin > i).then_some(fin)
        }
    }
}

#[cfg(test)]
mod essais {
    use super::*;
    use serde_json::json;

    /// Un dossier d'essai neuf, sous le dossier temporaire : jamais celui d'une vraie Eve.
    fn dossier(nom: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("eve-reglages-{nom}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    /// Un reglages.json d'essai qui dit `texte`, et les réglages qu'Eve en tire.
    fn ouvrir(nom: &str, texte: &str) -> (PathBuf, Reglages) {
        let chemin = dossier(nom).join("reglages.json");
        fs::write(&chemin, texte).unwrap();
        (chemin.clone(), charger_de(chemin))
    }

    /// `texte` écrit dans le fichier comme d'ailleurs (à la main, par l'Assistante), daté de `secondes` plus tard :
    /// deux écritures de suite peuvent tomber sur la même date, au pas de l'horloge du disque.
    fn ecrire_ailleurs(chemin: &Path, texte: &str, secondes: u64) {
        fs::write(chemin, texte).unwrap();
        let f = File::options().write(true).open(chemin).unwrap();
        f.set_modified(SystemTime::now() + Duration::from_secs(secondes)).unwrap();
    }

    /// [`Reglages::relire`] sans attendre la seconde d'une vérification à l'autre : deux vérifications de suite, comme
    /// deux secondes de suite (une version neuve se lit à la seconde).
    fn relire_maintenant(r: &mut Reglages) -> Option<Reglages> {
        r.fichier.verifie = None;
        if let Some(avant) = r.relire() {
            return Some(avant);
        }
        r.fichier.verifie = None;
        r.relire()
    }

    #[test]
    fn au_chaud_est_faux_d_office() {
        assert!(!Reglages::default().au_chaud, "chaque tour silencieux se paie : la personne le demande");
        assert!(texte_d_office().contains("\"au_chaud\": false"));
    }

    /// Un réglage changé dans la fenêtre ne change que lui : les champs d'une version plus récente d'Eve, l'ordre, les
    /// fins de ligne de Windows, la marque d'ordre des octets restent ; un réglage absent s'ajoute au bout de son objet,
    /// comme ses voisins.
    #[test]
    fn un_reglage_change_ne_touche_rien_d_autre() {
        let texte = "\u{feff}{\r\n    \"venu_d_ailleurs\": {\"liste\": [1, \"}\\\"{\"]},\r\n    \"sourdine\": false,\r\n    \
                     \"nom\": \"Essai\",\r\n    \"alertes\": { \"factures\": true }\r\n}\r\n";
        let (chemin, mut r) = ouvrir("ne-touche-rien", texte);
        assert!(r.illisible().is_none() && !r.sourdine);
        r.changer("sourdine", json!(true)).unwrap();
        assert!(r.sourdine);
        let attendu = texte.replace("\"sourdine\": false", "\"sourdine\": true");
        assert_eq!(fs::read_to_string(&chemin).unwrap(), attendu);
        r.changer("alertes.pieces", json!(true)).unwrap();
        r.changer("voix", json!("windows:Microsoft Caroline")).unwrap();
        let attendu = attendu
            .replace("{ \"factures\": true }", "{ \"factures\": true, \"pieces\": true }")
            .replace("}\r\n}\r\n", "},\r\n    \"voix\": \"windows:Microsoft Caroline\"\r\n}\r\n");
        assert_eq!(fs::read_to_string(&chemin).unwrap(), attendu);
        assert!(r.alertes.pieces && r.voix == "windows:Microsoft Caroline");
        // Une valeur que le fichier a déjà (ici, celle d'office) ne s'écrit pas.
        r.changer("interruption", json!(true)).unwrap();
        assert_eq!(fs::read_to_string(&chemin).unwrap(), attendu);
    }

    /// Ce qu'on change dans le fichier pendant qu'Eve tourne (à la main, l'installateur, l'Assistante) se garde quand la
    /// personne change un réglage : il s'inscrit dans le fichier relu, et la mémoire reprend le tout.
    #[test]
    fn un_changement_fait_ailleurs_se_garde() {
        let (chemin, mut r) = ouvrir("ailleurs", "{\n  \"voix\": \"piper:a\",\n  \"sourdine\": false\n}\n");
        let ailleurs = "{\n  \"voix\": \"piper:b\",\n  \"sourdine\": false,\n  \"privileges\": { \"chrome\": true },\n  \
                        \"plus_tard\": [\"un champ d'une version plus récente\"]\n}\n";
        ecrire_ailleurs(&chemin, ailleurs, 5);
        let avant = r.clone();
        r.changer("sourdine", json!(true)).unwrap();
        assert_eq!(fs::read_to_string(&chemin).unwrap(), ailleurs.replace("\"sourdine\": false", "\"sourdine\": true"));
        assert_eq!(r.voix, "piper:b", "la mémoire reprend le fichier");
        assert_eq!(changements(&avant, &r), ["privileges.chrome", "sourdine", "voix"]);
        assert!(au_prochain_demarrage("privileges.chrome") && au_prochain_demarrage("privileges"));
        assert!(!au_prochain_demarrage("voix") && !au_prochain_demarrage("alertes.factures"));
        assert!(relire_maintenant(&mut r).is_none(), "ce qu'Eve a écrit ne revient pas comme un changement");
    }

    /// Le fichier relu quand il change : une version neuve attend d'être restée la même d'une vérification à l'autre,
    /// puis la mémoire la reprend ; retiré, ce sont les valeurs d'office.
    #[test]
    fn relire_reprend_le_fichier() {
        let (chemin, mut r) = ouvrir("relire", "{\"sourdine\": false}");
        assert!(relire_maintenant(&mut r).is_none(), "rien n'a changé");
        ecrire_ailleurs(&chemin, "{\"sourdine\": true, \"theme\": \"constructo\"}", 5);
        r.fichier.verifie = None;
        assert!(r.relire().is_none(), "vue une fois : elle s'écrit peut-être encore");
        r.fichier.verifie = None;
        let avant = r.relire().expect("restée la même : elle se lit");
        assert_eq!(changements(&avant, &r), ["sourdine", "theme"]);
        assert!(r.sourdine && r.theme == "constructo");
        fs::remove_file(&chemin).unwrap();
        assert!(relire_maintenant(&mut r).is_some());
        assert!(!r.sourdine && r.theme == "centre", "retiré : les valeurs d'office");
    }

    /// Un reglages.json illisible — une faute de syntaxe, une valeur du mauvais type, un texte qui n'est pas en UTF-8 —
    /// n'est jamais écrasé : Eve prend les valeurs d'office en mémoire, et rien ne s'y écrit tant qu'il ne se lit pas.
    #[test]
    fn un_fichier_illisible_n_est_jamais_ecrase() {
        let cas: [(&str, &[u8]); 3] = [
            ("syntaxe", b"{\"sourdine\": true,, \"privileges\": {\"outils\": [\"Read\"]}}"),
            ("type", b"{\"sourdine\": \"oui\"}"),
            ("latin1", b"{\"nom\": \"Assistante \xe9t\xe9\", \"sourdine\": true}"),
        ];
        for (nom, octets) in cas {
            let chemin = dossier(&format!("illisible-{nom}")).join("reglages.json");
            fs::write(&chemin, octets).unwrap();
            let mut r = charger_de(chemin.clone());
            assert!(r.illisible().is_some(), "{nom}");
            assert!(!r.sourdine && r.privileges.outils.is_none(), "{nom} : les valeurs d'office, en mémoire");
            assert!(r.changer("eveil", json!(false)).is_err(), "{nom}");
            assert!(r.eveil, "{nom} : rien n'a changé");
            assert!(relire_maintenant(&mut r).is_none(), "{nom}");
            assert_eq!(fs::read(&chemin).unwrap(), octets, "{nom} : le fichier n'a pas bougé");
        }
    }

    /// Un fichier qui cesse de se lire pendant qu'Eve tourne : la mémoire garde ce qu'elle avait, rien ne s'y écrit ;
    /// réparé, il se relit.
    #[test]
    fn un_fichier_qui_ne_se_lit_plus_se_relit_repare() {
        let (chemin, mut r) = ouvrir("repare", "{\"sourdine\": true}");
        ecrire_ailleurs(&chemin, "{\"sourdine\": tru", 5);
        let avant = relire_maintenant(&mut r).expect("il ne se lit plus : la personne doit le savoir");
        assert!(avant.illisible().is_none() && r.illisible().is_some());
        assert!(r.sourdine, "la mémoire garde ce qu'elle avait");
        assert!(r.changer("eveil", json!(false)).is_err());
        assert_eq!(fs::read_to_string(&chemin).unwrap(), "{\"sourdine\": tru");
        ecrire_ailleurs(&chemin, "{\"sourdine\": false}", 10);
        let avant = relire_maintenant(&mut r).expect("réparé, il se relit");
        assert!(avant.illisible().is_some() && r.illisible().is_none() && !r.sourdine);
    }

    /// Le premier lancement écrit les valeurs d'office ; un fichier retiré pendant qu'Eve tourne revient au premier
    /// réglage changé.
    #[test]
    fn un_fichier_absent_prend_les_valeurs_d_office() {
        let chemin = dossier("absent").join("reglages.json");
        let mut r = charger_de(chemin.clone());
        assert_eq!(fs::read_to_string(&chemin).unwrap(), texte_d_office());
        fs::remove_file(&chemin).unwrap();
        r.changer("sourdine", json!(true)).unwrap();
        let lu: Value = serde_json::from_str(&fs::read_to_string(&chemin).unwrap()).unwrap();
        assert_eq!((&lu["sourdine"], &lu["voix"]), (&json!(true), &json!(VOIX_PAR_DEFAUT)));
    }

    /// Seul un réglage que ce programme connaît s'écrit d'ici, avec le bon type : jamais un privilège.
    #[test]
    fn un_privilege_ne_s_ecrit_pas_d_ici() {
        let (chemin, mut r) = ouvrir("privilege", "{\"sourdine\": false}");
        assert!(r.changer("privileges.chrome", json!(true)).is_err());
        assert!(r.changer("sourdne", json!(true)).is_err());
        assert!(r.changer("sourdine", json!("oui")).is_err());
        assert_eq!(fs::read_to_string(&chemin).unwrap(), "{\"sourdine\": false}");
    }

    /// L'inscription dans le texte : une chaîne qui ressemble à du JSON ne trompe pas, un nom écrit avec un échappement
    /// se reconnaît, un objet vide ou absent reçoit le réglage sur sa ligne, un fichier serré le reste.
    #[test]
    fn inscrire_dans_le_texte() {
        let texte = "{\n  \"note\": \"{\\\"sourdine\\\": true}, [\",\n  \"sourd\\u0069ne\": false,\n  \"fin\": null\n}";
        assert_eq!(inscrire(texte, "sourdine", &json!(true)).unwrap(), texte.replace("false", "true"));
        assert_eq!(inscrire("{}", "sourdine", &json!(true)).unwrap(), "{\n  \"sourdine\": true\n}");
        assert_eq!(
            inscrire("{\n  \"nom\": \"Essai\"\n}", "alertes.pieces", &json!(true)).unwrap(),
            "{\n  \"nom\": \"Essai\",\n  \"alertes\": {\n    \"pieces\": true\n  }\n}"
        );
        assert_eq!(
            inscrire("{\"nom\":\"Essai\"}", "qui", &json!("Julie")).unwrap(),
            "{\"nom\":\"Essai\",\"qui\":\"Julie\"}"
        );
        assert!(inscrire("{\"alertes\": 3}", "alertes.pieces", &json!(true)).is_none());
        assert!(inscrire("[1]", "sourdine", &json!(true)).is_none());
    }

    /// Une écriture d'un seul geste : interrompue, elle laisse l'ancien fichier entier ; un fichier tenu ouvert ailleurs
    /// sans partage n'est pas remplacé.
    #[test]
    fn une_ecriture_interrompue_laisse_l_ancien_fichier_entier() {
        let d = dossier("interrompue");
        let (chemin, a_cote) = (d.join("memoire.json"), d.join("memoire.json.tmp"));
        fs::write(&chemin, "l'ancien, entier").unwrap();
        // Une coupure au milieu : le fichier à côté est resté à moitié écrit, l'ancien est intact…
        fs::write(&a_cote, "le nouv").unwrap();
        assert_eq!(fs::read_to_string(&chemin).unwrap(), "l'ancien, entier");
        // … et l'écriture suivante remplace ce reste, sans rien laisser à côté.
        ecrire_d_un_geste(&chemin, b"le nouveau").unwrap();
        assert_eq!(fs::read_to_string(&chemin).unwrap(), "le nouveau");
        assert!(!a_cote.exists());
        // Le fichier à côté ne peut pas s'écrire (un dossier a pris sa place) : rien ne change.
        fs::create_dir(&a_cote).unwrap();
        assert!(ecrire_d_un_geste(&chemin, b"perdu").is_err());
        assert_eq!(fs::read_to_string(&chemin).unwrap(), "le nouveau");
        fs::remove_dir(&a_cote).unwrap();
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            // Tenu ouvert en lecture, sans partage d'écriture ni de suppression (FILE_SHARE_READ seul), comme un
            // programme qui le garde : il n'est pas remplacé, et le fichier à côté s'efface.
            let tenu = fs::OpenOptions::new().read(true).share_mode(1).open(&chemin).unwrap();
            assert!(ecrire_d_un_geste(&chemin, b"perdu").is_err());
            drop(tenu);
            assert_eq!(fs::read_to_string(&chemin).unwrap(), "le nouveau");
            assert!(!a_cote.exists());
        }
    }

    #[test]
    fn les_privileges_completent_la_sorte_d_eve() {
        let mut r = Reglages::default();
        assert_eq!(r.espace(true), Espace::specialiste());
        assert_eq!(r.espace(false), Espace::adjointe());
        r.privileges =
            serde_json::from_str(r#"{"chrome": true, "outils": ["Read", " PowerShell ", "Bash(git *)", ""]}"#).unwrap();
        let e = r.espace(true);
        assert!(e.chrome && e.connecteurs && e.mcp_du_poste && e.constructo && e.accords, "le reste : la liberté");
        assert_eq!(
            e.outils,
            Some(vec!["Read".to_string(), "PowerShell".to_string()]),
            "un nom mal formé ne compte pas"
        );
        r.privileges.outils = Some(vec!["Tous".into()]);
        assert_eq!(r.espace(true).outils, None);
        assert!(r.espace(false).chrome, "un privilège donné vaut aussi pour l'Adjointe");
        assert!(!r.espace(false).constructo, "l'Adjointe n'a Constructo AI que si on le lui donne");
    }

    /// Un spécialiste installé avant la liberté totale garde ce que son reglages.json dit ; ce qu'il ne dit pas
    /// (Constructo AI, les accords) prend la valeur d'office — les accords restent donc, sans qu'on le décide.
    #[test]
    fn un_specialiste_d_avant_garde_ses_privileges() {
        let r: Reglages = serde_json::from_str(
            r#"{"nom": "Assistante famille", "privileges": {"connecteurs": false, "chrome": true, "mcp_du_poste": false,
                "outils": ["Read", "Write", "Edit", "Glob", "Grep", "WebSearch", "WebFetch", "PowerShell"]}}"#,
        )
        .unwrap();
        let e = r.espace(true);
        assert!(!e.connecteurs && e.chrome && !e.mcp_du_poste, "{e:?}");
        assert_eq!(e.outils.as_ref().map(Vec::len), Some(8));
        assert!(e.constructo && e.accords, "{e:?}");
        let libre: Reglages =
            serde_json::from_str(r#"{"privileges": {"accords": false, "constructo": false}}"#).unwrap();
        let e = libre.espace(true);
        assert!(!e.accords && !e.constructo && e.chrome, "{e:?}");
        assert!(!libre.espace(false).accords, "un privilège retiré vaut aussi pour l'Adjointe");
    }

    #[test]
    fn le_modele_de_la_carte_se_choisit() {
        let mut r = Reglages::default();
        r.oreille = r"D:\modeles\ggml-small-q5_1.bin".into();
        r.oreille_carte = r"D:\modeles\ggml-medium-q5_0.bin".into();
        assert_eq!(r.chemin_oreille_carte(), PathBuf::from(r"D:\modeles\ggml-medium-q5_0.bin"));
        // Sans modèle dit, celui des demandes, à moins que large-v3-turbo soit posé dans le dossier local.
        r.oreille_carte.clear();
        let turbo = ressource("modeles").join("ggml-large-v3-turbo-q5_0.bin");
        let attendu = if turbo.is_file() { turbo } else { PathBuf::from(r"D:\modeles\ggml-small-q5_1.bin") };
        assert_eq!(r.chemin_oreille_carte(), attendu);
        let ancien: Reglages = serde_json::from_str(r#"{"oreille": ""}"#).unwrap();
        assert!(ancien.oreille_carte.is_empty(), "un reglages.json d'avant se lit encore");
    }

    #[test]
    fn seuls_les_privileges_donnes_s_ecrivent() {
        let t = serde_json::to_string(&Reglages::default()).unwrap();
        assert!(t.contains(r#""privileges":{}"#), "{t}");
        let mut r = Reglages::default();
        r.privileges.mcp_du_poste = Some(true);
        let t = serde_json::to_string(&r).unwrap();
        assert!(t.contains(r#""privileges":{"mcp_du_poste":true}"#), "{t}");
        // Un reglages.json d'avant les privilèges se lit : la sorte d'Eve décide.
        let r: Reglages = serde_json::from_str(r#"{"nom": "Assistante famille"}"#).unwrap();
        assert_eq!(r.espace(true), Espace::specialiste());
    }

    /// La comptabilité de la fenêtre : aucune d'office, ni dans un reglages.json d'avant ; nommée, elle vaut tout de
    /// suite, et s'inscrit comme un autre réglage.
    #[test]
    fn la_comptabilite_vaut_tout_de_suite() {
        assert!(Reglages::default().comptabilite.is_empty());
        let ancien: Reglages = serde_json::from_str(r#"{"nom": "Essai"}"#).unwrap();
        assert!(ancien.comptabilite.is_empty());
        assert!(!au_prochain_demarrage("comptabilite"));
        let (chemin, mut r) = ouvrir("comptabilite", "{\"nom\": \"Essai\"}");
        r.changer("comptabilite", json!(r"D:\Exemple\Comptes")).unwrap();
        assert_eq!(r.comptabilite, r"D:\Exemple\Comptes");
        assert_eq!(fs::read_to_string(&chemin).unwrap(), r#"{"nom": "Essai","comptabilite": "D:\\Exemple\\Comptes"}"#);
    }

    /// Les mots qui désignent les modèles et les efforts d'Eve, sans égard à la casse.
    #[test]
    fn les_mots_du_cerveau() {
        for mot in ["opus", "Opus 5.5", " OPUS ", "claude-opus-5-5"] {
            assert_eq!(modele_dit(mot), Some("claude-opus-5-5"), "{mot}");
        }
        assert_eq!(modele_dit("Sonnet"), Some("claude-sonnet-5-5"));
        assert_eq!(modele_dit("haiku"), None);
        for (mot, effort) in [("Max", "max"), ("moyen", "medium"), ("medium", "medium")] {
            assert_eq!(effort_dit(mot), Some(effort), "{mot}");
        }
        assert_eq!(effort_dit("high"), None);
        assert_eq!(nom_du_modele("claude-opus-5-5"), "Opus 5.5");
        assert_eq!(nom_du_modele("claude-haiku-4-5"), "claude-haiku-4-5", "un modèle qu'Eve n'offre pas");
        assert_eq!((nom_de_l_effort("medium"), nom_de_l_effort("high")), ("moyen", "high"));
    }

    /// `eve --modele` et `eve --effort` n'écrivent que leurs réglages, dans le texte même du fichier ; un mot qu'Eve ne
    /// connaît pas n'écrit rien.
    #[test]
    fn changer_le_cerveau_n_ecrit_que_lui() {
        let (chemin, mut r) =
            ouvrir("cerveau", "{\r\n  \"nom\": \"Essai\",\r\n  \"modele\": \"claude-sonnet-5-5\"\r\n}\r\n");
        let m = changer_le_cerveau(&mut r, Some("Opus"), Some("max")).unwrap();
        assert!(m.starts_with("Le cerveau de cette Eve : Opus 5.5, effort max ;"), "{m}");
        let texte = fs::read_to_string(&chemin).unwrap();
        assert!(texte.starts_with("{\r\n  \"nom\": \"Essai\",\r\n  \"modele\": \"claude-opus-5-5\","), "{texte}");
        assert!(texte.contains("\"effort\": \"max\""), "{texte}");
        let e = changer_le_cerveau(&mut r, Some("haiku"), Some("moyen")).unwrap_err();
        assert!(e.contains("« haiku » n'est pas un modèle d'Eve") && e.contains("Sonnet 5.5, Opus 5.5"), "{e}");
        let e = changer_le_cerveau(&mut r, None, Some("high")).unwrap_err();
        assert!(e.contains("« high » n'est pas un effort d'Eve"), "{e}");
        assert_eq!(fs::read_to_string(&chemin).unwrap(), texte, "un refus n'écrit rien");
        changer_le_cerveau(&mut r, None, Some("moyen")).unwrap();
        let lu = charger_de(chemin);
        assert_eq!((lu.modele.as_str(), lu.effort.as_str()), ("claude-opus-5-5", "medium"));
    }
}
