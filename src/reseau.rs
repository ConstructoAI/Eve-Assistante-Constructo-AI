//! Le réseau des actions (le lot 6) : la pièce centrale du centre de commande de la fenêtre native. Un réseau
//! « neuronal » en 3D qui tourne lentement : Eve au centre ; autour, les destinations de ses outils (le dossier de
//! travail, un autre dossier du poste, le Web, Chrome, un serveur MCP, les commandes du poste…) ; entre les deux, des
//! neurones et leurs synapses. Quand Claude prend un outil, une impulsion part d'Eve vers sa destination et allume les
//! neurones au passage ; quand l'outil a fini, une autre en revient : verte, ou rouge s'il a échoué.
//!
//! Le modèle est la page que l'Assistante famille s'est faite (`centre-de-commande.html`, aux archives) :
//! sa correspondance des outils, son cerveau (le même tirage et la même graine : les mêmes 330 neurones), ses lueurs.
//! Elle avait sept destinations fixes ; un même programme sert ici tous les spécialistes, alors les destinations
//! naissent au fil des outils, 60 au plus (les suivantes se regroupent dans « Autres »). Le dessin passe par le Painter
//! d'egui : des cercles et des segments, les lueurs en couleurs additives (le `lighter` du canevas du modèle), la
//! perspective calculée à la main.
//!
//! Tout se calcule d'après l'heure, jamais d'après le nombre d'images : au repos, la fenêtre ne redessine que quatre
//! fois par seconde, et la rotation, lente, y reste juste. [`Reseau::dessiner`] dit quand il faut redessiner plus vite.
//!
//! Comme le modèle, le réseau pense. Quand un outil parle de l'école, d'une facture ou d'un courriel (ou que la
//! personne en parle, ou que le spécialiste l'écrit dans `sujet.js`), ses neurones quittent la sphère pour dessiner
//! l'image au trait (onze images, celles du modèle), de face, puis y retournent ; chaque personne du spécialiste a son
//! quartier du réseau, qui prend sa couleur quand un outil la nomme ; une couleur demandée teint tout le réseau ; la
//! voix le fait vibrer. Il tient aussi ce que la page écrit autour de lui : la ligne d'état, tapée lettre à lettre, et
//! la courbe d'activité ; et ses modes : la démonstration, la journée rejouée d'après le journal d'Eve, le mode
//! économe.

use std::cmp::Ordering;
use std::collections::{HashMap, VecDeque};
use std::f32::consts::{PI, TAU};
use std::path::{Path, PathBuf};
use std::time::Instant;

use eframe::egui::text::{LayoutJob, TextFormat, TextWrapping};
use eframe::egui::{
    self, Align2, Color32, CursorIcon, FontId, Mesh, Painter, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Vec2, pos2,
    vec2,
};
use serde_json::Value;

/// Les neurones du cerveau, comme le modèle.
const NEURONES: usize = 330;
/// Les destinations au plus, « Autres » compris : au-delà, ni les couleurs ni les étiquettes ne se distingueraient.
pub const MAX_DESTINATIONS: usize = 60;
/// La sphère des destinations, juste au-dehors du cerveau (ses neurones vont jusqu'à 1,02).
const RAYON_DESTINATIONS: f32 = 1.1;
/// La graine du modèle : le même tirage donne le même cerveau que sa page, d'une séance à l'autre.
const GRAINE: u32 = 20_261_023;
/// L'œil est à trois rayons du centre : la perspective du modèle.
const RECUL: f32 = 3.0;
/// La rotation au repos, en radians par seconde : celle du modèle (0,0008 par image à 30 images par seconde), un tour
/// en un peu plus de quatre minutes ; à quatre images par seconde, un pas d'un pixel ou deux.
const ROTATION: f64 = 0.024;
/// Le temps qu'une lueur met à perdre les deux tiers de son éclat (0,965 par image à 30 images par seconde).
const EXTINCTION: f64 = 0.94;
/// L'éclat d'une destination qu'une impulsion vient d'atteindre, et d'Eve quand une réponse lui revient.
const DUREE_ECLAT: f64 = 1.7;
/// Le cercle rouge d'une destination où un outil vient d'échouer.
const DUREE_ERREUR: f64 = 4.0;
/// Une destination qui apparaît grandit pendant ce temps.
const DUREE_NAISSANCE: f64 = 0.8;
/// Un neurone qui passe au lobe d'une destination nouvelle change de couleur pendant ce temps.
const DUREE_TEINTE: f64 = 1.2;
/// L'onde et les étincelles d'une arrivée.
const DUREE_ONDE: f64 = 1.1;
const DUREE_ETINCELLE: f64 = 1.33;
/// Un outil dont la fin n'est jamais venue (un tour interrompu) cesse de compter parmi les outils en cours.
const OUBLI: f64 = 900.0;
/// De quoi rester fluide quand Claude enchaîne les outils : au-delà, les plus anciennes s'effacent.
const MAX_IMPULSIONS: usize = 48;
const MAX_ONDES: usize = 32;
const MAX_ETINCELLES: usize = 480;
const MAX_AMBIANTES: usize = 25;
/// Au-delà de ce nombre de destinations, les étiquettes de l'arrière du réseau s'effacent : leur point suffit, et la
/// rotation les ramène devant.
const FOULE: usize = 14;
/// Les bandeaux du haut (le compte, la légende) et du bas (la dernière action), où les étiquettes ne se posent pas.
const BANDEAU: f32 = 26.0;

/// Les images que le réseau sait dessiner (cdc:163-184), et leur nom dans la ligne d'état (« PENSE : ÉCOLE »).
pub const IMAGES: [(&str, &str); 11] = [
    ("ecole", "ÉCOLE"),
    ("piece", "ARGENT"),
    ("lettre", "COURRIELS"),
    ("agenda", "AGENDA"),
    ("croix", "SANTÉ"),
    ("globe", "WEB"),
    ("repas", "REPAS"),
    ("maison", "MAISON"),
    ("avion", "AVION"),
    ("avions", "AVIONS"),
    ("soleil", "SOLEIL"),
];
/// Une image se défait après ce temps, à moins qu'on en demande un autre (cdc:215) ; une valeur de `sujet.js` plus
/// vieille que ceci ne joue pas (cdc:508).
const DUREE_SUJET: f64 = 15.0;
const AGE_MAX_SUJET_JS: f64 = 45.0;
/// Une personne nommée garde sa couleur ce temps (cdc:543) ; sa couleur vient et s'en va de 15 % de l'écart toutes les
/// 66 ms (cdc:545) : −ln(0,85) / 0,066 par seconde.
const DUREE_PERSONNE: f64 = 12.0;
const ALLUMAGE: f64 = 2.4625;
/// Les personnes, au plus : au-delà, leurs quartiers seraient trop minces pour se voir.
pub const MAX_PERSONNES: usize = 8;
/// Une couleur demandée seule dure ce temps, avec une image celui-là ; son retour se fond sur les trois dernières
/// secondes (cdc:509-511, 535).
const DUREE_COULEUR: f64 = 40.0;
const DUREE_COULEUR_AVEC_SUJET: f64 = 20.0;
const FONDU_COULEUR: f64 = 3.0;
/// La ligne d'état : une lettre toutes les 22 ms, le curseur qui clignote (0,8 s), « À L'ÉCOUTE » 1,5 s après le
/// dernier outil (cdc:20, 469-477, 503).
const FRAPPE: f64 = 0.022;
const CLIGNOTEMENT: f64 = 0.8;
const ECOUTE_APRES: f64 = 1.5;
const A_L_ECOUTE: &str = "EVE À L'ÉCOUTE";
/// Le temps qu'une ligne d'outil ou d'écoute reste lisible quand Eve change d'état (sa phrase de repos attend).
const LECTURE: f64 = 2.0;
/// Le curseur, au bout de la ligne d'état quand il se montre (une espace quand il se cache : la même chasse, la ligne
/// centrée ne bouge pas).
pub const CURSEUR: char = '▌';
/// La courbe d'activité : 90 cases de 0,7 s (cdc:251-252).
pub const CASES: usize = 90;
const CASE: f64 = 0.7;
/// La démonstration : un faux outil toutes les 0,8 s (cdc:824-828).
const DEMO_PAS: f64 = 0.8;
const EXEMPLES: usize = 8;
/// La journée rejouée : ses 150 dernières lignes (un début ou une fin), une toutes les 0,26 s (cdc:588, 595).
const MAX_REDIFFUSION: usize = 150;
const REDIFFUSION_PAS: f64 = 0.26;
/// Le mode économe se met de lui-même quand une image du réseau coûte en moyenne plus que ceci (en millisecondes de
/// processeur) pendant 120 images (cdc:771-773). Le modèle s'allégeait sous 22 images par seconde (45 ms l'image) ; le
/// réseau fait le dixième d'une image de la fenêtre (mesuré sur un Ivy Bridge de 2012 : 0,2 ms sur 2 ms) : 4 ms pour
/// lui, c'est une fenêtre qui ne tient plus ses 25 images par seconde.
const SEUIL_ECONOME: f64 = 4.0;

type Rgb = [u8; 3];

/// Eve et les requêtes qu'elle envoie ; les réponses ; les échecs.
const CYAN: Rgb = [0, 229, 255];
const VERT: Rgb = [124, 255, 107];
const ROUGE: Rgb = [255, 59, 78];
/// Le fond de nuit, plus clair au centre, et ses étoiles.
const FOND: Color32 = Color32::from_rgb(2, 5, 10);
const FOND_CENTRE: Color32 = Color32::from_rgb(10, 26, 48);
const ETOILE: Rgb = [160, 220, 255];
const ANNEAU: Color32 = Color32::from_rgba_unmultiplied_const(0, 229, 255, 23);
/// L'intérieur d'une destination, d'Eve, d'une étiquette.
const NUIT: Rgb = [6, 16, 31];
const NUIT_EVE: Color32 = Color32::from_rgb(4, 16, 30);
const ENCRE: Rgb = [3, 10, 20];
/// Les textes : vif, pâle, discret.
const BLANC: Rgb = [232, 251, 255];
const PALE: Rgb = [143, 184, 212];
const GRIS: Rgb = [95, 138, 168];
/// Les autres dossiers du poste en tons chauds, les serveurs MCP en tons vifs : une couleur chacun, dans l'ordre où ils
/// apparaissent (le premier dossier a l'ambre de la Comptabilité du modèle, le premier serveur le corail de
/// Constructo).
const DOSSIERS: [Rgb; 6] =
    [[255, 179, 71], [255, 210, 63], [255, 159, 67], [246, 193, 119], [255, 198, 165], [255, 224, 138]];
const SERVEURS: [Rgb; 6] =
    [[255, 122, 89], [255, 92, 138], [95, 255, 196], [195, 255, 90], [255, 143, 171], [160, 168, 255]];

/// Un outil de Claude, vu par le chef : son début, ou sa fin.
#[derive(Clone, Debug, Default)]
pub struct Evenement {
    /// L'identifiant de l'outil (`tool_use_id`) : il relie la fin à son début.
    pub id: String,
    pub outil: String,
    /// Ce que vise l'outil : le `file_path` de Read, Write ou Edit ; pour Glob et Grep, leur `path` s'il y en a un (il
    /// dit le dossier), sinon leur `pattern` ; l'`url` de WebFetch, la `query` de WebSearch, la `command` d'un shell.
    pub detail: String,
    pub fin: bool,
    /// Pour une fin : l'outil a échoué.
    pub erreur: bool,
}

/// Une personne du spécialiste (pour l'Assistante famille : les parents, les enfants), que la fenêtre lit dans son
/// profil : aucun prénom n'est écrit dans le programme. Son nom (tel que la ligne d'état le dit, « POUR … »), sa
/// couleur, et les mots qui la désignent dans un outil, son nom compris : « garderie » pour l'enfant qui y va.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Personne {
    pub nom: String,
    pub couleur: [u8; 3],
    pub motifs: Vec<String>,
}

impl Personne {
    /// Le texte (plié) la nomme : son nom ou l'un de ses mots, en mots entiers (« Léa » dans `Léa.md`, pas dans
    /// « Cléa » ni « Léandre »).
    fn nommee_dans(&self, plie: &str) -> bool {
        std::iter::once(&self.nom).chain(&self.motifs).any(|m| en_mots_entiers(plie, &plier(m.trim())))
    }
}

/// Les modes du réseau, que la page dit à sa façon (« MODE DÉMO », « REDIFFUSION DE LA JOURNÉE »).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modes {
    pub demo: bool,
    pub rediffusion: bool,
    pub econome: bool,
    /// Le mode économe s'est mis de lui-même : les images coûtaient trop.
    pub econome_de_lui_meme: bool,
    /// La rotation automatique est arrêtée (la touche R) : le réseau ne tourne plus que sous la main.
    pub immobile: bool,
}

/// Où va un outil : une destination du réseau.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Destination {
    /// Le dossier de travail du spécialiste : ses fichiers, et les motifs sans chemin (Claude y travaille).
    Travail,
    /// Un autre dossier du poste, par le dossier qui le nomme : `C:\Exemple\Comptabilité`, `D:\Photos`,
    /// `C:\Users\Sam\Documents`.
    Dossier(String),
    /// Le Web : la recherche et la lecture de pages.
    Web,
    /// Chrome, que Claude conduit (claude-in-chrome).
    Chrome,
    /// Un serveur MCP, par son nom (`claude_ai_Constructo_AI`, `freecad`).
    Mcp(String),
    /// Les courriels d'Outlook, qu'une commande ouvre.
    Outlook,
    /// Les autres commandes du poste (PowerShell, Bash).
    Commandes,
    /// Les outils de Claude Code lui-même : les compétences, les sous-agents, la recherche d'outils, les tâches…
    Outils,
    /// Les destinations au-delà de la cinquante-neuvième, regroupées. [`destination`] ne la rend jamais : le réseau y
    /// range le surplus.
    Autres,
}

impl Destination {
    /// Son nom à l'écran, en capitales comme le modèle ; `travail` : le dossier de travail.
    pub fn nom(&self, travail: &Path) -> String {
        match self {
            Destination::Travail => match Chemin::lire(&travail.to_string_lossy()).and_then(|mut c| c.parties.pop()) {
                Some(n) => format!("DOSSIER {}", n.to_uppercase()),
                None => "DOSSIER DE TRAVAIL".to_string(),
            },
            Destination::Dossier(d) => match d.rsplit('\\').find(|p| !p.is_empty()) {
                Some("~") | None => "PROFIL".to_string(),
                Some(n) => n.to_uppercase(),
            },
            Destination::Web => "WEB".to_string(),
            Destination::Chrome => "CHROME".to_string(),
            // Les connecteurs de claude.ai s'appellent claude_ai_<Nom> : le nom suffit.
            Destination::Mcp(s) => {
                let n = s.strip_prefix("claude_ai_").unwrap_or(s).replace(['_', '-'], " ");
                if n.trim().is_empty() { "MCP".to_string() } else { n.trim().to_uppercase() }
            }
            Destination::Outlook => "COURRIELS OUTLOOK".to_string(),
            Destination::Commandes => "POSTE · COMMANDES".to_string(),
            Destination::Outils => "OUTILS & COMPÉTENCES".to_string(),
            Destination::Autres => "AUTRES".to_string(),
        }
    }

    /// Ce qui la distingue d'une autre : deux chemins qui ne diffèrent que par la casse sont le même dossier (Windows).
    fn cle(&self) -> String {
        match self {
            Destination::Dossier(d) => format!("dossier:{}", d.to_lowercase()),
            Destination::Mcp(s) => format!("mcp:{s}"),
            autre => format!("{autre:?}"),
        }
    }

    /// Sa couleur ; `rang` : combien de destinations du même genre l'ont précédée (les dossiers, les serveurs).
    fn couleur(&self, rang: usize) -> Rgb {
        match self {
            Destination::Travail => CYAN,
            Destination::Dossier(_) => DOSSIERS[rang % DOSSIERS.len()],
            Destination::Web => [183, 140, 255],
            Destination::Chrome => [214, 140, 255],
            Destination::Mcp(_) => SERVEURS[rang % SERVEURS.len()],
            Destination::Outlook => [90, 169, 255],
            Destination::Commandes => VERT,
            Destination::Outils => [255, 79, 216],
            Destination::Autres => [143, 163, 184],
        }
    }

    fn meme_genre(&self, autre: &Destination) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(autre)
    }
}

/// La destination d'un outil (pure, pour les essais), d'après la page de l'Assistante famille : un connecteur ou un
/// serveur MCP par son nom (Chrome à part), le Web, les courriels d'Outlook ouverts par une commande, les autres
/// commandes, les fichiers par leur dossier ; le reste (compétences, sous-agents…) va aux outils de Claude Code.
pub fn destination(outil: &str, detail: &str, dossier_de_travail: &Path) -> Destination {
    if let Some(reste) = outil.strip_prefix("mcp__") {
        let serveur = reste.split("__").next().unwrap_or(reste);
        return match serveur {
            "" => Destination::Outils,
            "claude-in-chrome" => Destination::Chrome,
            s => Destination::Mcp(s.to_string()),
        };
    }
    match outil {
        "WebSearch" | "WebFetch" => Destination::Web,
        "Bash" | "PowerShell" | "Monitor" | "BashOutput" | "KillShell" | "KillBash" => {
            let d = detail.to_lowercase();
            if d.contains("outlook") || d.contains("mapi") { Destination::Outlook } else { Destination::Commandes }
        }
        "Read" | "Write" | "Edit" | "MultiEdit" | "NotebookEdit" | "NotebookRead" | "Glob" | "Grep" | "LS" => {
            dossier(outil, detail, dossier_de_travail)
        }
        _ => Destination::Outils,
    }
}

/// Un chemin absolu ramené à sa forme simple : sa racine (`C:`, `\\serveur\partage`, `~` pour le profil) et ses
/// parties, sans `.` ni `..`, les `/` et les `\` confondus.
#[derive(Clone, Debug, PartialEq)]
struct Chemin {
    racine: String,
    parties: Vec<String>,
}

impl Chemin {
    /// `None` pour un chemin relatif, ou pour un motif qui n'est pas un chemin.
    fn lire(texte: &str) -> Option<Chemin> {
        let brut = texte.trim().trim_matches(|c| c == '"' || c == '\'');
        // /c/Exemple/… : un chemin de Git Bash (avant que les / deviennent des \ : une expression régulière qui commence
        // par \b n'est pas un lecteur).
        if let Some(reste) = brut.strip_prefix('/') {
            let mut c = reste.chars();
            if let Some(l) = c.next()
                && l.is_ascii_alphabetic()
                && (c.as_str().is_empty() || c.as_str().starts_with('/'))
            {
                return Some(Chemin::de(format!("{}:", l.to_ascii_uppercase()), c.as_str()));
            }
        }
        let mut t = brut.replace('/', "\\");
        // Les préfixes des chemins longs de Windows : \\?\C:\…, \\?\UNC\serveur\…
        for (prefixe, devient) in [(r"\\?\UNC\", r"\\"), (r"\\.\UNC\", r"\\"), (r"\\?\", ""), (r"\\.\", "")] {
            if let Some(r) = t.strip_prefix(prefixe) {
                t = format!("{devient}{r}");
                break;
            }
        }
        if let Some(r) = t.strip_prefix(r"\\") {
            let mut m = r.splitn(3, '\\');
            let (serveur, partage) = (m.next().unwrap_or(""), m.next().unwrap_or(""));
            if !serveur.chars().next().is_some_and(char::is_alphanumeric) {
                return None;
            }
            return Some(Chemin::de(format!(r"\\{serveur}\{partage}"), m.next().unwrap_or("")));
        }
        let mut c = t.chars();
        if let (Some(l), Some(':')) = (c.next(), c.next())
            && l.is_ascii_alphabetic()
        {
            return Some(Chemin::de(format!("{}:", l.to_ascii_uppercase()), &t[2..]));
        }
        if t == "~" || t.starts_with("~\\") {
            return Some(Chemin::de("~".to_string(), &t[1..]));
        }
        None
    }

    fn de(racine: String, reste: &str) -> Chemin {
        let mut chemin = Chemin { racine, parties: Vec::new() };
        chemin.ajouter(reste);
        chemin
    }

    /// Ajoute des parties relatives ; un `..` remonte d'un dossier.
    fn ajouter(&mut self, reste: &str) {
        for p in reste.split(['\\', '/']) {
            match p {
                "" | "." => {}
                ".." => {
                    self.parties.pop();
                }
                p => self.parties.push(p.to_string()),
            }
        }
    }

    /// `self` est `autre` ou est dedans, sans égard à la casse.
    fn sous(&self, autre: &Chemin) -> bool {
        egal(&self.racine, &autre.racine)
            && self.parties.len() >= autre.parties.len()
            && self.parties.iter().zip(&autre.parties).all(|(a, b)| egal(a, b))
    }

    /// Le dossier fait de la racine et des `n` premières parties ; la racine seule d'un lecteur s'écrit `C:\`.
    fn texte(&self, n: usize) -> String {
        let mut s = self.racine.clone();
        for p in &self.parties[..n.min(self.parties.len())] {
            s.push('\\');
            s.push_str(p);
        }
        if s.ends_with(':') {
            s.push('\\');
        }
        s
    }
}

fn egal(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

/// La destination d'un chemin ou d'un motif : le dossier de travail s'il y est (un chemin relatif, un motif sans
/// chemin y sont : Claude y travaille), sinon le dossier qui le nomme.
fn dossier(outil: &str, detail: &str, dossier_de_travail: &Path) -> Destination {
    let travail = Chemin::lire(&dossier_de_travail.to_string_lossy());
    let mut chemin = match (Chemin::lire(detail), &travail) {
        (Some(c), _) => c,
        // Le motif de Grep est une expression régulière : relatif, il n'a rien d'un chemin.
        (None, Some(t)) if outil != "Grep" => {
            let mut c = t.clone();
            c.ajouter(detail.trim());
            c
        }
        (None, _) => return Destination::Travail,
    };
    // Le dossier, sans le fichier (Read, Write…) ni le motif (Glob : C:\Exemple\Comptabilité\**\*.pdf).
    if let Some(k) = chemin.parties.iter().position(|p| p.contains(['*', '?', '[', '{'])) {
        chemin.parties.truncate(k);
    } else if matches!(outil, "Read" | "Write" | "Edit" | "MultiEdit" | "NotebookEdit" | "NotebookRead")
        || chemin.parties.last().is_some_and(|p| semble_un_fichier(p))
    {
        chemin.parties.pop();
    }
    if let Some(t) = &travail
        && chemin.sous(t)
    {
        return Destination::Travail;
    }
    Destination::Dossier(nommer(&chemin, travail.as_ref()))
}

/// Le dossier qui nomme un chemin hors du dossier de travail : sous le profil, son premier dossier (`Documents`), et
/// sous `AppData`, celui du programme (`AppData\Local\Eve`) ; un voisin du dossier de travail, par son nom
/// (`C:\Exemple\Comptabilité` pour `C:\Exemple\Maison`) ; sous `Program Files`, le programme ; sinon le premier dossier
/// sous la racine (`D:\Photos`).
fn nommer(c: &Chemin, travail: Option<&Chemin>) -> String {
    let p = &c.parties;
    let est = |i: usize, nom: &str| p.get(i).is_some_and(|x| x.eq_ignore_ascii_case(nom));
    let profil = if c.racine == "~" {
        Some(0)
    } else if est(0, "Users") && p.len() >= 2 {
        Some(2)
    } else {
        None
    };
    let n = if let Some(base) = profil {
        if est(base, "AppData") { base + 3 } else { base + 1 }
    } else if let Some(t) = travail
        && egal(&c.racine, &t.racine)
        && !t.parties.is_empty()
        && p.len() >= t.parties.len()
        && p.iter().zip(&t.parties[..t.parties.len() - 1]).all(|(a, b)| egal(a, b))
    {
        t.parties.len()
    } else if est(0, "Program Files") || est(0, "Program Files (x86)") || est(0, "ProgramData") {
        2
    } else {
        1
    };
    c.texte(n)
}

/// Une partie qui finit par une extension (`plan.md`, `Eve.bat`) : un fichier. `.claude` n'en est pas un.
fn semble_un_fichier(partie: &str) -> bool {
    match partie.rfind('.') {
        Some(i) if i > 0 => {
            let ext = &partie[i + 1..];
            (1..=8).contains(&ext.len()) && ext.chars().all(|c| c.is_ascii_alphanumeric())
        }
        _ => false,
    }
}

/// Ce que fait un outil, en capitales comme le modèle ; d'un outil MCP, son nom seul (la destination dit le serveur).
fn verbe(outil: &str) -> String {
    let v = match outil {
        "Read" | "NotebookRead" => "LECTURE",
        "Write" => "ÉCRITURE",
        "Edit" | "MultiEdit" | "NotebookEdit" => "MODIFICATION",
        "Glob" | "LS" => "RECHERCHE DE FICHIERS",
        "Grep" => "RECHERCHE DE TEXTE",
        "Bash" | "PowerShell" | "Monitor" | "BashOutput" => "COMMANDE",
        "WebSearch" => "RECHERCHE WEB",
        "WebFetch" => "LECTURE WEB",
        "ToolSearch" => "CHARGEMENT D'OUTIL",
        "Agent" | "Task" => "SOUS-AGENT",
        "Skill" => "COMPÉTENCE",
        "TodoWrite" => "TÂCHES",
        _ => {
            let nom = outil.strip_prefix("mcp__").map_or(outil, |r| r.split_once("__").map_or(r, |(_, o)| o));
            return nom.replace(['_', '-'], " ").trim().to_uppercase();
        }
    };
    v.to_string()
}

/// Ce que porte une impulsion : le fichier, le site, sinon le début de la commande, du motif ou de la recherche.
fn etiquette(outil: &str, detail: &str) -> String {
    let d = detail.split_whitespace().collect::<Vec<_>>().join(" ");
    let texte = if let Some(h) = hote(&d) {
        h.to_string()
    } else if d.is_empty() {
        verbe(outil).to_lowercase()
    } else if matches!(outil, "Read" | "Write" | "Edit" | "MultiEdit" | "NotebookEdit" | "NotebookRead" | "Glob" | "LS")
    {
        d.rsplit(['\\', '/']).find(|p| !p.is_empty()).unwrap_or(&d).to_string()
    } else {
        d
    };
    raccourcir(&texte, 26)
}

/// Le nom d'hôte d'une adresse web, sans `www.` : `https://www.ecoles.exemple.ca/x` donne `ecoles.exemple.ca`.
fn hote(texte: &str) -> Option<&str> {
    let reste = texte.strip_prefix("https://").or_else(|| texte.strip_prefix("http://"))?;
    let hote = &reste[..reste.find(['/', '?', '#', ' ']).unwrap_or(reste.len())];
    let hote = hote.rsplit('@').next().unwrap_or(hote);
    let hote = hote.split(':').next().unwrap_or(hote);
    let hote = hote.strip_prefix("www.").unwrap_or(hote);
    (!hote.is_empty()).then_some(hote)
}

/// Au plus `max` caractères ; ce qui dépasse devient « … ».
fn raccourcir(texte: &str, max: usize) -> String {
    if texte.chars().count() <= max {
        return texte.to_string();
    }
    let mut s: String = texte.chars().take(max.saturating_sub(1)).collect();
    s.push('…');
    s
}

/// Une destination du réseau, telle qu'il la dessine.
struct Pole {
    destination: Destination,
    cle: String,
    nom: String,
    couleur: Rgb,
    /// Sa place parmi les points du réseau.
    place: usize,
    nee: f64,
    total: u32,
    en_cours: u32,
    erreurs: u32,
    /// L'heure de la dernière impulsion arrivée, de la dernière erreur.
    eclat: f64,
    erreur: f64,
    /// La dernière action, pour la bulle : « LECTURE · C:\Exemple\Maison\agenda.md ».
    derniere: String,
    /// Le côté où son étiquette s'est posée à l'image d'avant : dessous, dessus, à droite, à gauche (0 à 3).
    cote: u8,
}

/// Une impulsion qui court le long des synapses, d'Eve à une destination (l'aller) ou retour.
struct Impulsion {
    chemin: Vec<usize>,
    depart: f64,
    /// En synapses par seconde.
    vitesse: f32,
    couleur: Rgb,
    etiquette: String,
    pole: usize,
    aller: bool,
    /// Les points du chemin déjà allumés au passage.
    allumes: usize,
}

/// L'onde d'une arrivée, ou d'une destination qui naît.
struct Onde {
    place: usize,
    depart: f64,
    couleur: Rgb,
}

/// Une étincelle d'arrivée ; sa course se calcule d'après son âge.
struct Etincelle {
    place: usize,
    depart: f64,
    direction: Vec2,
    /// Au départ, en points par seconde.
    vitesse: f32,
    couleur: Rgb,
}

/// L'activité spontanée du modèle : un signal qui court le long d'une synapse, le temps que Claude travaille.
struct Ambiante {
    de: usize,
    vers: usize,
    depart: f64,
    duree: f64,
}

/// Le lobe d'un neurone : la destination la plus proche (sa couleur), celle d'avant, et depuis quand.
#[derive(Clone, Copy)]
struct Lobe {
    pole: usize,
    avant: usize,
    depuis: f64,
}

/// La dernière action, en toutes lettres au bas du réseau.
struct Derniere {
    texte: String,
    pole: usize,
    heure: f64,
}

/// L'image à laquelle le réseau pense, et jusqu'à quand.
struct Sujet {
    nom: &'static str,
    jusqu_a: f64,
}

/// L'image qu'il dessine : la place de chaque neurone sur ses traits.
struct Forme {
    nom: &'static str,
    cibles: Vec<[f32; 2]>,
}

/// L'éclat d'une personne nommée : sa force à l'instant `depuis`, et jusqu'à quand il monte.
#[derive(Clone, Copy)]
struct Eclairage {
    niveau: f32,
    depuis: f64,
    jusqu_a: f64,
}

impl Eclairage {
    const ETEINT: Eclairage = Eclairage { niveau: 0.0, depuis: 0.0, jusqu_a: f64::NEG_INFINITY };

    /// Sa force à l'instant `t`, de 0 à 1 : elle monte vers 1 tant que la personne est nommée, puis redescend.
    fn force(&self, t: f64) -> f32 {
        let monte =
            |jusqu: f64| 1.0 - (1.0 - f64::from(self.niveau)) * (-ALLUMAGE * (jusqu - self.depuis).max(0.0)).exp();
        let f = if t <= self.jusqu_a {
            monte(t)
        } else if self.jusqu_a >= self.depuis {
            monte(self.jusqu_a) * (-ALLUMAGE * (t - self.jusqu_a)).exp()
        } else {
            f64::from(self.niveau) * (-ALLUMAGE * (t - self.depuis).max(0.0)).exp()
        };
        f as f32
    }
}

/// La journée rejouée : ce qui reste à rejouer, et quand vient la ligne suivante.
struct Rediffusion {
    file: VecDeque<Evenement>,
    prochaine: f64,
}

/// L'état du réseau d'une image à l'autre : sa rotation, ses impulsions en vol, ses nœuds allumés.
pub struct Reseau {
    travail: PathBuf,
    /// Les points du réseau : Eve (0), les neurones (1 à `NEURONES`), puis les places des destinations.
    places: Vec<[f32; 3]>,
    voisins: Vec<Vec<usize>>,
    synapses: Vec<(usize, usize)>,
    /// Les places des destinations dans l'ordre où elles se donnent : chacune au plus loin des précédentes.
    ordre: Vec<usize>,
    /// La destination posée sur chaque point, s'il y en a une.
    pole_de: Vec<Option<usize>>,
    /// La lueur de chaque point : son éclat, et l'heure où il l'a reçu (elle s'éteint d'elle-même).
    lueurs: Vec<(f32, f64)>,
    lobes: Vec<Lobe>,
    poles: Vec<Pole>,
    /// Les outils en cours, par identifiant : leur destination, et depuis quand.
    en_cours: HashMap<String, (usize, f64)>,
    impulsions: Vec<Impulsion>,
    ondes: Vec<Onde>,
    etincelles: Vec<Etincelle>,
    ambiantes: Vec<Ambiante>,
    etoiles: Vec<[f32; 3]>,
    hasard: Hasard,
    /// La rotation autour de l'axe vertical (elle s'accumule, d'où le f64), l'inclinaison, l'élan laissé par un geste,
    /// le rapprochement de la molette.
    lacet: f64,
    tangage: f32,
    elan: f64,
    zoom: f32,
    avant: Option<f64>,
    /// La part d'étincelle spontanée pas encore tirée (elles se tirent au rythme du temps, pas des images).
    reliquat: f64,
    total: u32,
    eclat_eve: f64,
    /// Les neurones changent de couleur jusqu'à cette heure.
    teintes: f64,
    derniere: Option<Derniere>,
    /// Les points projetés de l'image en cours.
    projetes: Vec<Projete>,
    /// Le réseau dessine son nœud « EVE », et son cadre (les compteurs, la légende, la dernière action) : la fenêtre
    /// qui pose l'orbe au centre et son propre cadre les éteint ([`Reseau::habiller`]).
    montrer_eve: bool,
    montrer_cadre: bool,
    /// L'image voulue, l'image dessinée, et où en est le dessin (0 : la sphère, 1 : l'image).
    sujet: Option<Sujet>,
    forme: Option<Forme>,
    morph: f32,
    /// L'inclinaison à retrouver quand l'image se défait : le réseau s'est mis de face pour la montrer.
    tangage_repos: Option<f32>,
    /// Les mots que la fenêtre ajoute à ceux qui font penser à une image (les prénoms des enfants, pour l'école).
    evocations: Vec<(&'static str, Vec<String>)>,
    personnes: Vec<Personne>,
    eclairages: Vec<Eclairage>,
    /// Le quartier de chaque point du réseau : le rang de sa personne.
    quartiers: Vec<u8>,
    /// La couleur demandée, et jusqu'à quand.
    couleur: Option<(Rgb, f64)>,
    /// La voix, lissée, et le niveau que la fenêtre vient de donner.
    voix: f32,
    voix_donnee: f32,
    /// L'instant (`t`) de la valeur de `sujet.js` déjà jouée.
    sujet_js_vu: Option<f64>,
    /// La ligne d'état et l'heure où elle a commencé à se taper ; l'heure où la phrase de repos la remplacera, et cette
    /// phrase (« EVE À L'ÉCOUTE » d'office : [`Reseau::repos`]).
    ligne: String,
    ligne_depuis: f64,
    a_l_ecoute: Option<f64>,
    repos: String,
    /// La courbe d'activité : les signaux de chaque case, et le numéro de la dernière (l'heure divisée par 0,7 s).
    cases: [u16; CASES],
    case: i64,
    econome: bool,
    econome_seul: bool,
    immobile: bool,
    /// Ce que coûte une image en moyenne (ms), et combien d'images ont coûté trop.
    cout_moyen: f64,
    lentes: u32,
    /// La démonstration : l'heure du prochain faux outil (inconnue juste après l'avoir lancée), leur nombre, et les
    /// fins à venir.
    demo: Option<f64>,
    demo_n: u32,
    simulees: Vec<(f64, Evenement)>,
    rediffusion: Option<Rediffusion>,
}

/// Ce qui vaut pour toute une image : le rectangle, la caméra, la taille des choses (petite dans une petite fenêtre).
struct Vue {
    rect: Rect,
    camera: Camera,
    taille: f32,
}

impl Reseau {
    /// `dossier_de_travail` : celui du spécialiste (C:\Exemple\Maison pour l'Assistante famille), pour reconnaître ses
    /// fichiers parmi les chemins des outils.
    pub fn nouveau(dossier_de_travail: &Path) -> Reseau {
        let Cerveau { places, voisins, synapses, ordre } = construire();
        let n = places.len();
        let mut hasard = Hasard(GRAINE ^ 0x00E5_FF00);
        let etoiles = (0..110).map(|_| [hasard.f(), hasard.f(), hasard.f()]).collect();
        let mut reseau = Reseau {
            travail: dossier_de_travail.to_path_buf(),
            places,
            voisins,
            synapses,
            ordre,
            pole_de: vec![None; n],
            lueurs: vec![(0.0, f64::NEG_INFINITY); n],
            lobes: vec![Lobe { pole: 0, avant: 0, depuis: f64::NEG_INFINITY }; n],
            poles: Vec::new(),
            en_cours: HashMap::new(),
            impulsions: Vec::new(),
            ondes: Vec::new(),
            etincelles: Vec::new(),
            ambiantes: Vec::new(),
            etoiles,
            hasard,
            lacet: 0.6,
            tangage: -0.35,
            elan: 0.0,
            zoom: 1.0,
            avant: None,
            reliquat: 0.0,
            total: 0,
            eclat_eve: f64::NEG_INFINITY,
            teintes: f64::NEG_INFINITY,
            derniere: None,
            projetes: Vec::with_capacity(n),
            montrer_eve: true,
            montrer_cadre: true,
            sujet: None,
            forme: None,
            morph: 0.0,
            tangage_repos: None,
            evocations: Vec::new(),
            personnes: Vec::new(),
            eclairages: Vec::new(),
            quartiers: vec![0; n],
            couleur: None,
            voix: 0.0,
            voix_donnee: 0.0,
            sujet_js_vu: None,
            // Déjà tapée : la page s'ouvre sur elle.
            ligne: A_L_ECOUTE.to_string(),
            ligne_depuis: f64::NEG_INFINITY,
            a_l_ecoute: None,
            repos: A_L_ECOUTE.to_string(),
            cases: [0; CASES],
            case: 0,
            econome: false,
            econome_seul: false,
            immobile: false,
            cout_moyen: 0.0,
            lentes: 0,
            demo: None,
            demo_n: 0,
            simulees: Vec::new(),
            rediffusion: None,
        };
        // Le dossier de travail est là d'emblée : c'est la maison du spécialiste.
        reseau.pole(Destination::Travail, f64::NEG_INFINITY);
        reseau
    }

    /// Le réseau dessine-t-il son nœud « EVE » (sinon, une orbe s'y pose : [`Reseau::centre_de`]) et son cadre (les
    /// compteurs, la légende, la dernière action : [`Reseau::compteurs`], [`Reseau::derniere`]) ?
    pub fn habiller(&mut self, eve: bool, cadre: bool) {
        self.montrer_eve = eve;
        self.montrer_cadre = cadre;
    }

    /// Le centre du réseau dans `rect`, où est Eve : là que la fenêtre pose l'orbe.
    pub fn centre_de(rect: Rect) -> Pos2 {
        rect.center() + vec2(0.0, 4.0)
    }

    /// Le nombre d'actions depuis l'ouverture, et celles en cours.
    pub fn compteurs(&self) -> (u32, usize) {
        (self.total, self.en_cours.len())
    }

    /// Les personnes du spécialiste, huit au plus : chacune a son quartier du réseau (la sphère coupée autour de son
    /// axe ; à quatre, les quartiers du modèle), qui prend sa couleur 12 s quand un outil la nomme, et la ligne d'état
    /// dit alors « POUR … » (cdc:529-545).
    pub fn personnes(&mut self, mut liste: Vec<Personne>) {
        liste.truncate(MAX_PERSONNES);
        let n = liste.len();
        self.quartiers = self.places.iter().map(|p| secteur(p[0], p[2], n) as u8).collect();
        self.eclairages = vec![Eclairage::ETEINT; n];
        self.personnes = liste;
    }

    /// Des mots de plus qui font penser à une image, avant ceux du modèle : pour `ecole`, les prénoms des enfants et le
    /// nom de leur groupe, que le modèle avait dans son code (cdc:188). Ils valent en mots entiers ; une liste vide
    /// les retire ; une image inconnue ne change rien.
    pub fn evoquer(&mut self, image: &str, motifs: Vec<String>) {
        let Some(image) = image_connue(image) else { return };
        let motifs: Vec<String> = motifs.iter().map(|m| plier(m.trim())).filter(|m| !m.is_empty()).collect();
        self.evocations.retain(|(i, _)| *i != image);
        if !motifs.is_empty() {
            self.evocations.push((image, motifs));
        }
    }

    /// Le réseau pense à une image ([`IMAGES`]) pendant `duree` secondes : ses neurones la dessinent, puis retournent à
    /// la sphère ; `aucun` la défait tout de suite. Faux pour une image inconnue.
    pub fn sujet(&mut self, nom: &str, duree: f64, temps: f64) -> bool {
        if nom.trim() == "aucun" {
            self.sujet = None;
            return true;
        }
        let Some(nom) = image_connue(nom) else { return false };
        self.sujet = Some(Sujet { nom, jusqu_a: temps + duree.max(0.0) });
        true
    }

    /// La valeur de `sujet.js` (`window.EVE_SUJET = {…}`, cdc:506-513), que la fenêtre relit chaque seconde : `s` une
    /// image ou `aucun`, `c` une couleur [r, v, b], `d` la durée en millisecondes, `t` l'instant en secondes Unix, `p`
    /// une personne (son rang) ou une liste. Une valeur ne joue qu'une fois, et pas si elle a plus de 45 s. Une couleur
    /// seule teint le réseau 40 s (ou `d`) ; avec une image, 20 s, et l'image dure 15 s (ou `d`).
    pub fn sujet_js(&mut self, valeur: &Value, maintenant_unix: f64, temps: f64) {
        let Some(t) = valeur.get("t").and_then(Value::as_f64) else { return };
        if maintenant_unix - t > AGE_MAX_SUJET_JS || self.sujet_js_vu == Some(t) {
            return;
        }
        let s = match valeur.get("s") {
            None | Some(Value::Null) => "",
            Some(Value::String(s)) => s.trim(),
            Some(_) => return,
        };
        let couleur = valeur.get("c").and_then(rgb_de);
        let duree = valeur.get("d").and_then(Value::as_f64).filter(|d| *d > 0.0).map(|d| d / 1000.0);
        let personnes: Vec<usize> = match valeur.get("p") {
            Some(Value::Array(a)) => a.iter().filter_map(Value::as_u64).map(|k| k as usize).collect(),
            Some(v) => v.as_u64().map(|k| k as usize).into_iter().collect(),
            None => Vec::new(),
        };
        if s.is_empty() {
            let Some(c) = couleur else { return };
            self.teinte(c, duree.unwrap_or(DUREE_COULEUR), temps);
            self.ecouter("EVE ÉCOUTE · TOUT EN COULEUR".to_string(), temps);
        } else if s == "aucun" {
            self.sujet = None;
        } else if let Some(image) = image_connue(s) {
            if let Some(c) = couleur {
                self.teinte(c, DUREE_COULEUR_AVEC_SUJET, temps);
            }
            self.sujet(image, duree.unwrap_or(DUREE_SUJET), temps);
            self.ecouter(format!("EVE ÉCOUTE · IMAGINE : {}", nom_d_image(image)), temps);
        } else {
            return;
        }
        self.sujet_js_vu = Some(t);
        if s != "aucun" {
            for k in personnes {
                self.eclairer(k, temps);
            }
        }
    }

    /// Ce que la personne vient de dire (la transcription d'Eve, qui remplace la dictée du modèle, cdc:687-692) : qui y
    /// est nommé s'allume, et le réseau imagine ce dont elle parle.
    pub fn entendu(&mut self, texte: &str, temps: f64) {
        self.nommer(texte, temps);
        if let Some(s) = self.imaginer(texte, None) {
            self.sujet(s, DUREE_SUJET, temps);
            self.ecouter(format!("EVE ÉCOUTE · IMAGINE : {}", nom_d_image(s)), temps);
        }
    }

    /// Une phrase qui se tape sur la ligne d'état, comme ce que le réseau imagine : celle de la fête (cdc:789).
    pub fn annoncer(&mut self, texte: String, temps: f64) {
        self.ecouter(texte, temps);
    }

    /// Tout le réseau prend cette couleur pendant `duree` secondes (88 % de la couleur, cdc:535), et la quitte en se
    /// fondant sur les trois dernières.
    pub fn teinte(&mut self, rgb: [u8; 3], duree: f64, temps: f64) {
        self.couleur = Some((rgb, temps + duree.max(0.0)));
    }

    /// Le niveau du micro, de 0 à 1 (celui que publie le chef) : quand on parle, le réseau vibre (cdc:225).
    pub fn voix(&mut self, niveau: f32) {
        self.voix_donnee = if niveau.is_finite() { niveau.clamp(0.0, 1.0) } else { 0.0 };
    }

    /// La ligne d'état (cdc:467-477), telle qu'elle s'est tapée jusqu'ici, une lettre toutes les 22 ms : « EVE
    /// CONSULTE LE DOSSIER MAISON · famille.md · POUR … · PENSE : ÉCOLE ». Son dernier caractère est le curseur :
    /// [`CURSEUR`], ou une espace quand il clignote.
    pub fn ligne_d_etat(&self, temps: f64) -> String {
        let (texte, depuis) = self.ligne_a(temps);
        let n = texte.chars().count();
        let tapees = ((temps - depuis) / FRAPPE).floor().clamp(0.0, n as f64) as usize;
        let mut ligne: String = texte.chars().take(tapees).collect();
        // Le curseur reste allumé tant que la ligne se tape, puis clignote (le CSS du modèle, steps(1)).
        let fin = depuis + n as f64 * FRAPPE;
        let depuis_la_fin = if fin.is_finite() { temps - fin } else { temps };
        let allume = depuis_la_fin < 0.0 || depuis_la_fin.rem_euclid(CLIGNOTEMENT) < CLIGNOTEMENT / 2.0;
        ligne.push(if allume { CURSEUR } else { ' ' });
        ligne
    }

    /// La phrase de repos, que la ligne d'état tape 1,5 s après le dernier outil : « EVE À L'ÉCOUTE » d'office, comme
    /// le modèle ; la fenêtre y met ce que fait Eve (« EVE S'EN OCCUPE », « EVE TE RÉPOND »). Elle remplace tout de
    /// suite l'ancienne phrase de repos ; une ligne d'outil ou d'écoute reste lisible deux secondes après s'être tapée.
    pub fn repos(&mut self, texte: &str, temps: f64) {
        if texte == self.repos {
            return;
        }
        let au_repos = self.ligne == self.repos;
        self.repos = texte.to_string();
        if self.a_l_ecoute.is_none() {
            let tapee = self.ligne_depuis + self.ligne.chars().count() as f64 * FRAPPE;
            self.a_l_ecoute = Some(if au_repos { temps } else { temps.max(tapee + LECTURE) });
        }
    }

    /// L'image à laquelle le réseau pense (pour la gamme de la musique, cdc:285), à jour à chaque image dessinée.
    pub fn sujet_en_cours(&self) -> Option<&str> {
        self.sujet.as_ref().map(|s| s.nom)
    }

    /// La courbe d'activité (cdc:434-439) : les signaux partis et revenus dans chacune des 90 dernières cases de 0,7 s,
    /// de la plus ancienne à celle en cours, rapportés à la plus haute (trois au moins), de 0 à 1.
    pub fn activite(&self, temps: f64) -> Vec<f32> {
        let d = ((temps / CASE).floor() as i64 - self.case).clamp(0, CASES as i64) as usize;
        let haut = f32::from(self.cases[d..].iter().copied().max().unwrap_or(0).max(3));
        self.cases[d..].iter().map(|&c| f32::from(c) / haut).chain(std::iter::repeat_n(0.0, d)).collect()
    }

    /// Le mode économe, à la main (cdc:274) : une synapse sur deux, moins d'activité spontanée. Il se met aussi de
    /// lui-même quand les images coûtent trop (cdc:770-774).
    pub fn econome(&mut self, oui: bool) {
        self.econome = oui;
        self.econome_seul = false;
        self.lentes = 0;
    }

    /// La démonstration (cdc:815-829) : un faux outil toutes les 0,8 s (ceux du modèle, dans le dossier de travail),
    /// qui finit de 1,3 à 2,2 s plus tard. Arrêtée, les outils déjà partis finissent quand même.
    pub fn demo(&mut self, oui: bool) {
        match (oui, self.demo) {
            (true, None) => self.demo = Some(f64::NAN),
            (false, _) => self.demo = None,
            _ => {}
        }
    }

    /// Rejoue la journée (cdc:585-597) : les 150 dernières lignes de [`journee`] (un début ou une fin), une toutes les
    /// 0,26 s, les compteurs remis à zéro ; une liste vide arrête la rediffusion. Les outils en direct passent
    /// toujours.
    pub fn rejouer(&mut self, evenements: Vec<Evenement>, temps: f64) {
        if evenements.is_empty() {
            self.rediffusion = None;
            return;
        }
        self.total = 0;
        self.en_cours.clear();
        self.simulees.clear();
        for p in &mut self.poles {
            p.total = 0;
            p.en_cours = 0;
            p.erreurs = 0;
            p.derniere.clear();
        }
        let debut = evenements.len().saturating_sub(MAX_REDIFFUSION);
        self.rediffusion = Some(Rediffusion { file: evenements.into_iter().skip(debut).collect(), prochaine: temps });
    }

    pub fn modes(&self) -> Modes {
        Modes {
            demo: self.demo.is_some(),
            rediffusion: self.rediffusion.is_some(),
            econome: self.econome,
            econome_de_lui_meme: self.econome_seul,
            immobile: self.immobile,
        }
    }

    /// La rotation automatique (la touche R, cdc:276) : lente, elle ralentit quand le réseau dessine ; arrêtée, le
    /// réseau ne tourne plus que sous la main (et l'élan d'un geste).
    pub fn rotation(&mut self, oui: bool) {
        self.immobile = !oui;
    }

    /// Un outil commence ou finit : une impulsion part vers sa destination (une erreur en rouge). `temps` : la même
    /// horloge que [`Reseau::dessiner`].
    pub fn recevoir(&mut self, e: &Evenement, temps: f64) {
        self.oublier(temps);
        if e.fin {
            let p = match self.en_cours.remove(&e.id) {
                Some((p, _)) => {
                    self.poles[p].en_cours = self.poles[p].en_cours.saturating_sub(1);
                    p
                }
                // Une fin sans début (un début d'avant ce réseau) : une action de plus.
                None => self.compter(e, temps),
            };
            if e.erreur {
                self.poles[p].erreurs += 1;
                self.poles[p].erreur = temps;
            }
            // Un retour réussi n'a pas besoin de mot : sa couleur le dit.
            let (couleur, mot) = if e.erreur { (ROUGE, "ERREUR") } else { (VERT, "") };
            self.lancer(p, false, couleur, mot.to_string(), temps);
            // « À L'ÉCOUTE » 1,5 s après le dernier outil, s'il n'en vient pas d'autre d'ici là (cdc:503).
            if self.en_cours.is_empty() {
                self.a_l_ecoute = Some(temps + ECOUTE_APRES);
            }
        } else {
            let p = self.compter(e, temps);
            self.poles[p].en_cours += 1;
            if let Some((ancien, _)) = self.en_cours.insert(e.id.clone(), (p, temps)) {
                // Le même identifiant une seconde fois : le premier début ne finira plus.
                self.poles[ancien].en_cours = self.poles[ancien].en_cours.saturating_sub(1);
            }
            let etiquette = etiquette(&e.outil, &e.detail);
            let v = verbe(&e.outil);
            let texte = if e.detail.trim().is_empty() { v } else { format!("{v} · {etiquette}") };
            self.derniere = Some(Derniere { texte, pole: p, heure: temps });
            self.lancer(p, true, CYAN, etiquette, temps);
            self.penser_a(e, p, temps);
        }
    }

    /// Un outil commence (cdc:492) : qui il nomme s'allume, le réseau pense à ce dont il parle, et la ligne d'état dit
    /// ce qu'Eve fait, pour qui, et à quoi elle pense.
    fn penser_a(&mut self, e: &Evenement, pole: usize, temps: f64) {
        // Le détail, et le nom d'un outil MCP (« lister_factures ») : il dit ce que l'outil fait.
        let texte = match e.outil.strip_prefix("mcp__").and_then(|r| r.split_once("__")) {
            Some((_, nom)) => format!("{} {nom}", e.detail),
            None => e.detail.clone(),
        };
        let nommees = self.nommer(&texte, temps);
        if let Some(s) = self.imaginer(&texte, Some(&self.poles[pole].destination)) {
            self.sujet(s, DUREE_SUJET, temps);
        }
        // Le modèle mettait deux espaces autour des points qu'il ajoutait ; sa page les montrait comme une (le HTML
        // réduit les blancs) : une seule ici.
        let mut ligne = format!("EVE {}", self.phrase(pole, &e.outil));
        if !e.detail.trim().is_empty() {
            ligne += &format!(" · {}", etiquette(&e.outil, &e.detail));
        }
        if !nommees.is_empty() {
            let noms: Vec<String> = nommees.iter().map(|&k| self.personnes[k].nom.trim().to_uppercase()).collect();
            ligne += &format!(" · POUR {}", noms.join(" + "));
        }
        if let Some(s) = &self.sujet {
            ligne += &format!(" · PENSE : {}", nom_d_image(s.nom));
        }
        self.taper(ligne, temps);
        self.a_l_ecoute = None;
    }

    /// Ce que fait Eve, pour la ligne d'état (cdc:474-475) : « CONSULTE LE DOSSIER MAISON », « INTERROGE CONSTRUCTO
    /// AI »… Dans un autre dossier, elle le lit, ou y écrit.
    fn phrase(&self, pole: usize, outil: &str) -> String {
        let p = &self.poles[pole];
        match &p.destination {
            Destination::Travail => format!("CONSULTE LE {}", p.nom),
            Destination::Dossier(_) if matches!(outil, "Write" | "Edit" | "MultiEdit" | "NotebookEdit") => {
                format!("ÉCRIT DANS LE DOSSIER {}", p.nom)
            }
            Destination::Dossier(_) => format!("LIT LE DOSSIER {}", p.nom),
            Destination::Web => "CHERCHE SUR LE WEB".to_string(),
            Destination::Chrome => "NAVIGUE DANS CHROME".to_string(),
            Destination::Mcp(_) => format!("INTERROGE {}", p.nom),
            Destination::Outlook => "OUVRE LES COURRIELS OUTLOOK".to_string(),
            Destination::Commandes => "EXÉCUTE UNE COMMANDE SUR LE POSTE".to_string(),
            Destination::Outils => "PRÉPARE UN OUTIL".to_string(),
            Destination::Autres => "CONSULTE UNE AUTRE SOURCE".to_string(),
        }
    }

    /// Les personnes que nomme un texte s'allument : leurs rangs.
    fn nommer(&mut self, texte: &str, temps: f64) -> Vec<usize> {
        let plie = plier(texte);
        let nommees: Vec<usize> = (0..self.personnes.len()).filter(|&k| self.personnes[k].nommee_dans(&plie)).collect();
        for &k in &nommees {
            self.eclairer(k, temps);
        }
        nommees
    }

    fn eclairer(&mut self, k: usize, temps: f64) {
        if let Some(e) = self.eclairages.get_mut(k) {
            *e = Eclairage { niveau: e.force(temps), depuis: temps, jusqu_a: temps + DUREE_PERSONNE };
        }
    }

    /// L'image à laquelle fait penser un texte : [`sujet_de`], avec les mots qu'a ajoutés la fenêtre.
    fn imaginer(&self, texte: &str, destination: Option<&Destination>) -> Option<&'static str> {
        sujet_de(&plier(texte), &self.evocations, destination)
    }

    /// La ligne d'état change (le même texte ne se retape pas, cdc:470).
    fn taper(&mut self, texte: String, temps: f64) {
        if texte != self.ligne {
            self.ligne = texte;
            self.ligne_depuis = temps;
        }
    }

    /// Une ligne d'écoute (ce que le réseau imagine, la couleur demandée) : plus récente que le dernier outil, « À
    /// L'ÉCOUTE » ne la remplace pas. (Le minuteur du modèle l'effaçait quand il tombait après elle.)
    fn ecouter(&mut self, texte: String, temps: f64) {
        self.taper(texte, temps);
        self.a_l_ecoute = None;
    }

    /// La ligne d'état à l'instant `temps`, et l'heure où elle a commencé à se taper : « À L'ÉCOUTE » est peut-être
    /// déjà dû.
    fn ligne_a(&self, temps: f64) -> (&str, f64) {
        match self.a_l_ecoute {
            Some(t0) if temps >= t0 && self.ligne != self.repos => (&self.repos, t0),
            _ => (&self.ligne, self.ligne_depuis),
        }
    }

    /// Ce qui vient à son heure, avant chaque image : « À L'ÉCOUTE », les faux outils de la démonstration et leurs
    /// fins, les lignes de la journée rejouée. Après une longue absence (la fenêtre cachée), la démonstration et la
    /// rediffusion reprennent où elles en étaient, sans rattraper le temps perdu.
    fn jouer(&mut self, temps: f64) {
        if let Some(t0) = self.a_l_ecoute
            && temps >= t0
        {
            self.a_l_ecoute = None;
            self.taper(self.repos.clone(), t0);
        }
        if let Some(mut prochain) = self.demo {
            if !(prochain >= temps - 2.0) {
                prochain = temps + DEMO_PAS;
            }
            while prochain <= temps {
                self.demo_n += 1;
                let k = self.hasard.indice(EXEMPLES);
                let (outil, detail) = self.exemple(k);
                let e = Evenement { id: format!("demo{}", self.demo_n), outil, detail, fin: false, erreur: false };
                self.recevoir(&e, prochain);
                let fin = prochain + f64::from(self.hasard.entre(1.3, 2.2));
                self.simulees.push((fin, Evenement { fin: true, ..e }));
                prochain += DEMO_PAS;
            }
            self.demo = Some(prochain);
        }
        if self.simulees.iter().any(|(t, _)| *t <= temps) {
            let mut dues = Vec::new();
            self.simulees.retain(|s| {
                let due = s.0 <= temps;
                if due {
                    dues.push(s.clone());
                }
                !due
            });
            dues.sort_by(|a, b| a.0.total_cmp(&b.0));
            for (t, e) in dues {
                self.recevoir(&e, t);
            }
        }
        if let Some(mut r) = self.rediffusion.take() {
            if r.prochaine < temps - 2.0 {
                r.prochaine = temps;
            }
            while r.prochaine <= temps
                && let Some(e) = r.file.pop_front()
            {
                self.recevoir(&e, r.prochaine);
                r.prochaine += REDIFFUSION_PAS;
            }
            if !r.file.is_empty() {
                self.rediffusion = Some(r);
            }
        }
    }

    /// Un faux outil de la démonstration : ceux du modèle (cdc:817-819), dans le dossier de travail et à côté.
    fn exemple(&self, k: usize) -> (String, String) {
        let travail = self.travail.to_string_lossy();
        let base = travail.trim_end_matches(['\\', '/']);
        let dans = |nom: &str| if base.is_empty() { nom.to_string() } else { format!(r"{base}\{nom}") };
        let voisin = match base.rfind(['\\', '/']) {
            Some(i) if i > 0 => format!(r"{}\Comptabilité\plan-action.md", &base[..i]),
            _ => r"..\Comptabilité\plan-action.md".to_string(),
        };
        let (outil, detail) = match k {
            0 => ("Read", dans("notes.md")),
            1 => ("Read", voisin),
            2 => ("PowerShell", "Outlook.Application MAPI boîte de réception".to_string()),
            3 => ("WebSearch", "calendrier scolaire 2026-2027".to_string()),
            4 => ("Write", dans(r"agenda\ics\pedagogiques.ics")),
            5 => ("mcp__claude_ai_Constructo_AI__lister_factures", "factures".to_string()),
            6 => ("Grep", "journées pédagogiques".to_string()),
            _ => ("Skill", "courriels".to_string()),
        };
        (outil.to_string(), detail)
    }

    /// Le réseau pense (cdc:214-219, 343-344) : l'image voulue s'efface à son heure ; il ne change d'image qu'une fois
    /// revenu à la sphère ; le dessin s'approche de l'image ou s'en éloigne de 3 % de l'écart par image du modèle, plus
    /// un peu (il vient en 3 s, repart en 2,4 s) ; le réseau se met de face pour montrer l'image, puis retrouve son
    /// inclinaison ; la voix se lisse (30 % de l'écart toutes les 45 ms, cdc:609). `ecoule` : le temps depuis l'image
    /// d'avant, entier (même après une absence, le dessin est où il doit être).
    fn penser(&mut self, temps: f64, ecoule: f64, glisse: bool) {
        if self.sujet.as_ref().is_some_and(|s| temps >= s.jusqu_a) {
            self.sujet = None;
        }
        let voulu = self.sujet.as_ref().map(|s| s.nom);
        if voulu != self.forme.as_ref().map(|f| f.nom) && self.morph < 0.03 {
            self.forme = voulu.map(|nom| Forme { nom, cibles: cibles(nom, &self.places) });
        }
        // Le pas du modèle : 3 % de l'écart vers l'image (1) ou la sphère (0), et 0,002 de plus à l'aller, 0,004 au
        // retour ; ici sur le temps écoulé. (Le modèle tremblait d'un rien une fois l'image faite : il repartait de
        // 0,004 dès qu'il touchait 1 ; ici l'image arrivée reste faite.)
        let montre = voulu.is_some() && voulu == self.forme.as_ref().map(|f| f.nom);
        let vers = if montre { 1.0 + 0.002 / 0.03 } else { -0.004 / 0.03 };
        let images = (30.0 * ecoule) as f32;
        self.morph = (vers + (self.morph - vers) * 0.97f32.powf(images)).clamp(0.0, 1.0);
        if glisse {
            // La personne tourne le réseau : son inclinaison est la sienne.
            self.tangage_repos = None;
        } else if self.morph > 0.02 {
            let garde = (1.0 - 0.03 * f64::from(self.morph)).powf(30.0 * ecoule);
            let face = (self.lacet / std::f64::consts::TAU).round() * std::f64::consts::TAU;
            self.lacet = face + (self.lacet - face) * garde;
            if voulu.is_some() {
                self.tangage_repos.get_or_insert(self.tangage);
                self.tangage *= garde as f32;
            }
        }
        if voulu.is_none()
            && !glisse
            && let Some(repos) = self.tangage_repos
        {
            self.tangage = repos + (self.tangage - repos) * 0.97f32.powf(images);
            if (self.tangage - repos).abs() < 0.002 {
                self.tangage = repos;
                self.tangage_repos = None;
            }
        }
        self.voix = self.voix_donnee + (self.voix - self.voix_donnee) * 0.7f32.powf((ecoule / 0.045) as f32);
        if self.voix < 0.001 {
            self.voix = 0.0;
        }
    }

    /// La place d'un point à l'image en cours : un neurone glisse vers son point de l'image (à 95 %, la profondeur
    /// aplatie à 12 %), et vibre avec la voix (cdc:222-225). `m` : le dessin, adouci.
    fn place_vive(&self, i: usize, m: f32, temps: f64) -> [f32; 3] {
        let p = self.places[i];
        if i == 0 || i > NEURONES {
            return p;
        }
        let mut q = p;
        if m > 0.001
            && let Some(f) = &self.forme
        {
            let c = if f.nom == "avions" { avions(f.cibles[i], i, temps) } else { f.cibles[i] };
            q = [p[0] + (c[0] * 0.95 - p[0]) * m, p[1] + (c[1] * 0.95 - p[1]) * m, p[2] + (p[2] * 0.12 - p[2]) * m];
        }
        if self.voix > 0.02 {
            let a = 1.0 + self.voix * 0.22 * (temps * 11.0 + i as f64 * 0.9).sin() as f32;
            q = [q[0] * a, q[1] * a, q[2] * a];
        }
        q
    }

    /// Ce qu'a coûté l'image (en ms de processeur) : en moyenne plus que le seuil pendant 120 images, le réseau se met
    /// de lui-même en mode économe, comme le modèle (cdc:770-774) ; il y reste.
    fn mesurer(&mut self, ms: f64) {
        if ms > 250.0 {
            return;
        }
        self.cout_moyen = self.cout_moyen * 0.95 + ms * 0.05;
        if !self.econome && self.cout_moyen > SEUIL_ECONOME {
            self.lentes += 1;
            if self.lentes > 120 {
                self.econome = true;
                self.econome_seul = true;
            }
        }
    }

    /// Un signal de plus dans la case en cours de la courbe d'activité (cdc:324).
    fn compter_un_signal(&mut self, temps: f64) {
        let case = (temps / CASE).floor() as i64;
        let d = case - self.case;
        if d > 0 {
            let d = d.min(CASES as i64) as usize;
            self.cases.rotate_left(d);
            self.cases[CASES - d..].fill(0);
            self.case = case;
        }
        let c = &mut self.cases[CASES - 1];
        *c = c.saturating_add(1);
    }

    /// La couleur demandée, et sa force (88 %, qui se fond sur les trois dernières secondes), si elle dure encore.
    fn teinte_a(&self, temps: f64) -> Option<(Rgb, f32)> {
        let (c, fin) = self.couleur?;
        (temps < fin).then(|| (c, (((fin - temps) / FONDU_COULEUR).min(1.0) * 0.88) as f32))
    }

    /// La couleur d'un neurone (cdc:534-536) : celle de son lobe, teinte de la couleur demandée, puis mêlée de celle
    /// de la personne de son quartier quand elle est nommée ; et la force de celle-ci.
    fn couleur_neurone(&self, i: usize, temps: f64, teinte: Option<(Rgb, f32)>, forces: &[f32]) -> (Rgb, f32) {
        let mut c = self.couleur_lobe(i, temps);
        if let Some((t, k)) = teinte {
            c = melange(c, t, k);
        }
        let q = usize::from(self.quartiers[i]);
        let a = forces.get(q).copied().unwrap_or(0.0);
        if a >= 0.02
            && let Some(p) = self.personnes.get(q)
        {
            c = melange(c, p.couleur, a.min(1.0));
        }
        (c, a)
    }

    /// Le réseau dans le rectangle donné, à l'instant `temps` (en secondes : `ui.input(|i| i.time)`) ; rend vrai tant
    /// qu'il s'anime (la fenêtre redessine alors plus souvent). Glisser le fait tourner, la molette le rapproche, le
    /// survol d'une destination dit ce qu'elle a reçu.
    pub fn dessiner(&mut self, ui: &mut egui::Ui, rect: Rect, temps: f64) -> bool {
        let chrono = Instant::now();
        // La rotation avance d'au plus une demi-seconde (après une absence, elle ne saute pas) ; le dessin du sujet et
        // la voix, du temps entier.
        let ecoule = self.avant.map_or(0.0, |a| (temps - a).max(0.0));
        let dt = ecoule.min(0.5);
        self.avant = Some(temps);
        self.jouer(temps);
        self.oublier(temps);
        if !rect.is_positive() || !rect.is_finite() {
            return false;
        }
        let reponse = ui.interact(rect, ui.make_persistent_id("reseau-des-actions"), Sense::drag());
        let glisse = reponse.dragged();
        if glisse {
            // Le lâcher garde un élan, comme le modèle : les deux tiers de la vitesse du geste, qui s'éteignent en une
            // seconde ou deux.
            let d = reponse.drag_delta();
            self.lacet += f64::from(d.x) * 0.006;
            self.tangage = (self.tangage + d.y * 0.006).clamp(-1.35, 1.35);
            if dt > 0.0 {
                self.elan = (f64::from(d.x) * 0.006 / dt.max(1.0 / 120.0) * (2.0 / 3.0)).clamp(-3.0, 3.0);
            }
        } else {
            // Elle ralentit quand le réseau dessine (cdc:343).
            let seule = if self.immobile { 0.0 } else { ROTATION * f64::from(1.0 - self.morph) };
            self.lacet += (self.elan + seule) * dt;
            self.elan *= (-dt / 0.65).exp();
            if self.elan.abs() < 0.01 {
                self.elan = 0.0;
            }
        }
        if reponse.hovered() {
            ui.ctx().set_cursor_icon(if glisse { CursorIcon::Grabbing } else { CursorIcon::Grab });
            let molette = ui.input(|i| i.smooth_scroll_delta.y);
            if molette != 0.0 {
                self.zoom = (self.zoom * (molette * 0.002).exp()).clamp(0.55, 2.4);
                ui.ctx().input_mut(|i| i.smooth_scroll_delta.y = 0.0);
            }
        }
        let survol = reponse.hover_pos();
        self.penser(temps, ecoule, glisse);

        // Les destinations les plus loin projetées restent dans le rectangle (1,18 fois l'échelle, au plus).
        let echelle = (rect.width().min(rect.height()) * 0.5 - 28.0).max(30.0) / 1.2 * self.zoom;
        let camera = Camera {
            lacet: (self.lacet % std::f64::consts::TAU) as f32,
            tangage: self.tangage,
            centre: Reseau::centre_de(rect),
            echelle,
        };
        let vue = Vue { rect, camera, taille: (echelle / 330.0).clamp(0.6, 1.3) };
        let m = lisse(self.morph);
        let mut projetes = std::mem::take(&mut self.projetes);
        projetes.clear();
        projetes.extend((0..self.places.len()).map(|i| projeter(self.place_vive(i, m, temps), &vue.camera)));
        self.projetes = projetes;

        self.avancer(temps, dt);

        let p = ui.painter_at(rect);
        self.peindre_fond(&p, &vue, temps);
        self.peindre_anneaux(&p, &vue);
        self.peindre_synapses(&p, temps);
        self.peindre_ambiantes(&p, &vue, temps);
        self.peindre_neurones(&p, &vue, temps);
        self.peindre_impulsions(&p, &vue, temps);
        let (reperes, cotes) = self.peindre_poles(&p, &vue, temps);
        for (k, cote) in cotes {
            self.poles[k].cote = cote;
        }
        self.peindre_etiquettes_des_impulsions(&p, temps);
        self.peindre_eclats(&p, &vue, temps);
        if self.montrer_cadre {
            self.peindre_cadre(&p, &vue, temps);
        }
        if let Some(souris) = survol
            && !glisse
        {
            self.peindre_bulle(&p, &vue, &reperes, souris);
        }
        self.mesurer(chrono.elapsed().as_secs_f64() * 1000.0);
        self.s_anime(temps) || glisse || self.elan != 0.0
    }

    /// Le réseau bouge encore : une impulsion en vol, un outil en cours, un éclat qui s'éteint, une destination qui
    /// naît ; une image qu'il dessine ou qui se défait, une couleur, une personne allumée, la voix ; la démonstration,
    /// la journée rejouée ; la ligne d'état qui se tape.
    fn s_anime(&self, temps: f64) -> bool {
        let eclat = |t: f64| temps - t < DUREE_ECLAT;
        !self.impulsions.is_empty()
            || !self.ondes.is_empty()
            || !self.etincelles.is_empty()
            || !self.ambiantes.is_empty()
            || !self.en_cours.is_empty()
            || eclat(self.eclat_eve)
            || temps < self.teintes
            || self
                .poles
                .iter()
                .any(|q| eclat(q.eclat) || temps - q.erreur < DUREE_ERREUR || temps - q.nee < DUREE_NAISSANCE)
            || self.sujet.is_some()
            || self.morph > 0.0
            || self.tangage_repos.is_some()
            || self.voix > 0.0
            || self.voix_donnee > 0.02
            || self.couleur.is_some_and(|(_, fin)| temps < fin)
            || self.eclairages.iter().any(|e| temps < e.jusqu_a || e.force(temps) > 0.01)
            || self.demo.is_some()
            || !self.simulees.is_empty()
            || self.rediffusion.is_some()
            || self.a_l_ecoute.is_some()
            || temps < self.ligne_depuis + self.ligne.chars().count() as f64 * FRAPPE
    }

    /// Une action de plus vers la destination de l'outil (posée au besoin) : son numéro.
    fn compter(&mut self, e: &Evenement, temps: f64) -> usize {
        let d = destination(&e.outil, &e.detail, &self.travail);
        let p = self.pole(d, temps);
        self.total += 1;
        let pole = &mut self.poles[p];
        pole.total += 1;
        let detail = e.detail.split_whitespace().collect::<Vec<_>>().join(" ");
        let v = verbe(&e.outil);
        pole.derniere = raccourcir(&if detail.is_empty() { v } else { format!("{v} · {detail}") }, 160);
        p
    }

    /// Les outils sans fin depuis trop longtemps (un tour interrompu) ne comptent plus parmi les outils en cours.
    fn oublier(&mut self, temps: f64) {
        let poles = &mut self.poles;
        self.en_cours.retain(|_, &mut (p, depuis)| {
            let garde = temps - depuis < OUBLI;
            if !garde {
                poles[p].en_cours = poles[p].en_cours.saturating_sub(1);
            }
            garde
        });
    }

    /// Le numéro de la destination, posée sur la place suivante si elle est nouvelle (et qu'elle grandit, si `temps`
    /// n'est pas l'infini du départ).
    fn pole(&mut self, destination: Destination, temps: f64) -> usize {
        let cle = destination.cle();
        if let Some(p) = self.poles.iter().position(|p| p.cle == cle) {
            return p;
        }
        // La soixantième place est celle d'« Autres » : les destinations suivantes s'y regroupent.
        let (destination, cle) = if self.poles.len() + 1 >= MAX_DESTINATIONS {
            let autres = Destination::Autres;
            let cle = autres.cle();
            if let Some(p) = self.poles.iter().position(|p| p.cle == cle) {
                return p;
            }
            (autres, cle)
        } else {
            (destination, cle)
        };
        let rang = self.poles.iter().filter(|p| p.destination.meme_genre(&destination)).count();
        let numero = self.poles.len();
        let place = self.ordre[numero];
        let couleur = destination.couleur(rang);
        self.poles.push(Pole {
            nom: raccourcir(&destination.nom(&self.travail), 24),
            cle,
            couleur,
            place,
            destination,
            nee: temps,
            total: 0,
            en_cours: 0,
            erreurs: 0,
            eclat: f64::NEG_INFINITY,
            erreur: f64::NEG_INFINITY,
            derniere: String::new(),
            cote: 0,
        });
        self.pole_de[place] = Some(numero);
        self.recalculer_lobes(temps);
        if temps.is_finite() {
            self.ondes.push(Onde { place, depart: temps, couleur });
        }
        numero
    }

    /// Chaque neurone appartient au lobe de la destination la plus proche (le modèle) : une nouvelle venue prend sa
    /// part du cerveau, qui change de couleur en douceur.
    fn recalculer_lobes(&mut self, temps: f64) {
        for i in 1..=NEURONES {
            let p = self.places[i];
            let proche = (0..self.poles.len())
                .min_by(|&a, &b| {
                    d2(p, self.places[self.poles[a].place]).total_cmp(&d2(p, self.places[self.poles[b].place]))
                })
                .unwrap_or(0);
            let lobe = &mut self.lobes[i];
            if lobe.pole != proche {
                *lobe = Lobe { pole: proche, avant: lobe.pole, depuis: temps };
            }
        }
        self.teintes = temps + DUREE_TEINTE;
    }

    /// Une impulsion d'Eve vers la destination (l'aller), ou de la destination vers Eve ; la courbe d'activité la
    /// compte.
    fn lancer(&mut self, pole: usize, aller: bool, couleur: Rgb, etiquette: String, temps: f64) {
        self.compter_un_signal(temps);
        if self.impulsions.len() >= MAX_IMPULSIONS {
            self.impulsions.remove(0);
        }
        let cible = self.poles[pole].place;
        let mut chemin = self.chemin(cible);
        if !aller {
            chemin.reverse();
        }
        let vitesse = self.hasard.entre(2.6, 3.5);
        self.impulsions.push(Impulsion { chemin, depart: temps, vitesse, couleur, etiquette, pole, aller, allumes: 0 });
    }

    /// Un des plus courts chemins d'Eve à une place, par les neurones seulement, tiré au hasard parmi eux : le signal
    /// ne passe pas toujours au même endroit (le modèle).
    fn chemin(&mut self, cible: usize) -> Vec<usize> {
        let mut parent = vec![usize::MAX; self.places.len()];
        parent[0] = 0;
        let mut file = VecDeque::from([0]);
        let mut suivants = Vec::new();
        while let Some(u) = file.pop_front() {
            if u == cible {
                break;
            }
            suivants.clear();
            suivants.extend(self.voisins[u].iter().copied().filter(|&v| v <= NEURONES || v == cible));
            for k in (1..suivants.len()).rev() {
                let j = self.hasard.indice(k + 1);
                suivants.swap(k, j);
            }
            for &v in &suivants {
                if parent[v] == usize::MAX {
                    parent[v] = u;
                    file.push_back(v);
                }
            }
        }
        if parent[cible] == usize::MAX {
            return vec![0, cible];
        }
        let mut chemin = vec![cible];
        let mut u = cible;
        while u != 0 {
            u = parent[u];
            chemin.push(u);
        }
        chemin.reverse();
        chemin
    }

    fn lueur(&self, i: usize, temps: f64) -> f32 {
        let (eclat, depuis) = self.lueurs[i];
        if eclat <= 0.0 {
            return 0.0;
        }
        eclat * (-(temps - depuis).max(0.0) / EXTINCTION).exp() as f32
    }

    fn allumer(&mut self, i: usize, eclat: f32, temps: f64) {
        if eclat > self.lueur(i, temps) {
            self.lueurs[i] = (eclat, temps);
        }
    }

    fn naissance(&self, k: usize, temps: f64) -> f32 {
        let t = ((temps - self.poles[k].nee) / DUREE_NAISSANCE).clamp(0.0, 1.0) as f32;
        t * t * (3.0 - 2.0 * t)
    }

    fn couleur_lobe(&self, i: usize, temps: f64) -> Rgb {
        let lobe = self.lobes[i];
        let c = self.poles[lobe.pole].couleur;
        let k = ((temps - lobe.depuis) / DUREE_TEINTE).clamp(0.0, 1.0) as f32;
        if k >= 1.0 { c } else { melange(self.poles[lobe.avant].couleur, c, k) }
    }

    /// La couleur d'un point : Eve, sa destination, ou son lobe.
    fn couleur_place(&self, i: usize, temps: f64) -> Rgb {
        match (i, self.pole_de[i]) {
            (0, _) => CYAN,
            (_, Some(p)) => self.poles[p].couleur,
            _ => self.couleur_lobe(i, temps),
        }
    }

    /// Le temps passe : les impulsions avancent et allument les neurones au passage, celles qui arrivent font leur
    /// éclat ; les effets finis s'effacent ; le cerveau scintille.
    fn avancer(&mut self, temps: f64, dt: f64) {
        let mut allumes = Vec::new();
        let mut arrivees = Vec::new();
        let mut i = 0;
        while i < self.impulsions.len() {
            let imp = &mut self.impulsions[i];
            let longueur = (imp.chemin.len() - 1) as f64;
            let avance = ((temps - imp.depart) * f64::from(imp.vitesse)).max(0.0);
            let pas = (avance.floor() as usize).min(imp.chemin.len() - 1);
            while imp.allumes <= pas {
                allumes.push(imp.chemin[imp.allumes]);
                imp.allumes += 1;
            }
            if avance >= longueur {
                let arrivee = imp.depart + longueur / f64::from(imp.vitesse);
                arrivees.push((self.impulsions.remove(i), arrivee));
            } else {
                i += 1;
            }
        }
        for n in allumes {
            self.allumer(n, 1.0, temps);
        }
        for (imp, arrivee) in arrivees {
            self.arriver(imp, temps, arrivee);
        }
        self.ondes.retain(|o| temps - o.depart < DUREE_ONDE);
        self.etincelles.retain(|e| temps - e.depart < DUREE_ETINCELLE);
        let mut fins = Vec::new();
        self.ambiantes.retain(|m| {
            let fini = temps - m.depart >= m.duree;
            if fini {
                fins.push(m.vers);
            }
            !fini
        });
        for n in fins {
            self.allumer(n, 0.55, temps);
        }
        // L'activité spontanée : quand Claude travaille, des signaux courent les synapses (une douzaine par seconde) ;
        // au repos, quelques neurones s'allument (assez lents pour quatre images par seconde).
        let actif = !self.impulsions.is_empty() || !self.en_cours.is_empty();
        // En mode économe, huit signaux au plus en même temps (cdc:369).
        let plafond = if self.econome { 8 } else { MAX_AMBIANTES };
        self.reliquat += dt * if actif { 12.0 } else { 2.5 };
        while self.reliquat >= 1.0 {
            self.reliquat -= 1.0;
            if actif {
                let (a, b) = self.synapses[self.hasard.indice(self.synapses.len())];
                if a <= NEURONES && b <= NEURONES && self.ambiantes.len() < plafond {
                    let (de, vers) = if self.hasard.suivant() < 0.5 { (a, b) } else { (b, a) };
                    let duree = f64::from(self.hasard.entre(0.55, 1.1));
                    self.ambiantes.push(Ambiante { de, vers, depart: temps, duree });
                }
            } else {
                let n = 1 + self.hasard.indice(NEURONES);
                self.allumer(n, 0.5, temps);
            }
        }
    }

    /// Une impulsion arrive (à l'heure `arrivee`, un peu avant l'image) : la destination, ou Eve, s'éclaire ; la
    /// décharge gagne les voisins puis leurs voisins ; une onde et seize étincelles partent du point d'arrivée (le
    /// modèle). Une arrivée vieille de plus d'une seconde (la fenêtre était cachée) passe sans éclat.
    fn arriver(&mut self, imp: Impulsion, temps: f64, arrivee: f64) {
        let Some(&ou) = imp.chemin.last() else { return };
        if imp.aller {
            self.poles[imp.pole].eclat = arrivee;
        } else {
            self.eclat_eve = arrivee;
        }
        if temps - arrivee >= 1.0 {
            return;
        }
        self.allumer(ou, 1.0, temps);
        for k in 0..self.voisins[ou].len() {
            let v = self.voisins[ou][k];
            self.allumer(v, 0.9, temps);
            for m in 0..self.voisins[v].len() {
                let w = self.voisins[v][m];
                self.allumer(w, 0.45, temps);
            }
        }
        self.ondes.push(Onde { place: ou, depart: arrivee, couleur: imp.couleur });
        if self.ondes.len() > MAX_ONDES {
            self.ondes.remove(0);
        }
        for _ in 0..16 {
            let direction = Vec2::angled(self.hasard.entre(0.0, TAU));
            let vitesse = self.hasard.entre(30.0, 126.0);
            self.etincelles.push(Etincelle { place: ou, depart: arrivee, direction, vitesse, couleur: imp.couleur });
        }
        let trop = self.etincelles.len().saturating_sub(MAX_ETINCELLES);
        self.etincelles.drain(..trop);
    }

    /// La nuit, plus claire au centre (un dégradé en éventail, faute de dégradé radial), et ses étoiles, qui glissent
    /// un peu quand le réseau tourne.
    fn peindre_fond(&self, p: &Painter, v: &Vue, temps: f64) {
        let rect = v.rect;
        p.rect_filled(rect, 0.0, FOND);
        let centre = v.camera.centre;
        let rayon = rect.width().max(rect.height()) * 0.7;
        let mut m = Mesh::default();
        m.colored_vertex(centre, FOND_CENTRE);
        const COTES: u32 = 48;
        for k in 0..COTES {
            m.colored_vertex(centre + Vec2::angled(k as f32 / COTES as f32 * TAU) * rayon, FOND);
        }
        for k in 0..COTES {
            m.add_triangle(0, 1 + k, 1 + (k + 1) % COTES);
        }
        p.add(Shape::mesh(m));
        let (l, h) = (f64::from(rect.width()), f64::from(rect.height()));
        for e in &self.etoiles {
            let (x, y, z) = (f64::from(e[0]), f64::from(e[1]), f64::from(e[2]));
            let ex = (x * l + self.lacet * 60.0 * z).rem_euclid(l) as f32;
            let ey = (y * h + f64::from(self.tangage) * 40.0 * z).rem_euclid(h) as f32;
            let scintille = (temps * 2.0 + x * 40.0).sin() as f32;
            let force = 0.12 + 0.3 * e[2] * (0.6 + 0.4 * scintille);
            p.rect_filled(
                Rect::from_min_size(rect.min + vec2(ex, ey), Vec2::splat(1.0 + e[2])),
                0.0,
                lumiere(ETOILE, force),
            );
        }
    }

    /// Trois anneaux en orbite : les repères de la rotation. Vu par la tranche (le réseau de face pour montrer une
    /// image : l'anneau de l'équateur), un anneau est presque une ligne qui fait demi-tour à ses bouts ; il se dessine
    /// alors segment par segment, car egui joint les segments d'un tracé en onglet, et un onglet de demi-tour s'étire
    /// à l'infini.
    fn peindre_anneaux(&self, p: &Painter, v: &Vue) {
        for (a, b) in [(0.0f32, 0.0f32), (1.05, 0.2), (0.5, 1.2)] {
            let ((sa, ca), (sb, cb)) = (a.sin_cos(), b.sin_cos());
            let points: Vec<Pos2> = (0..80)
                .map(|i| {
                    let (s, c) = (i as f32 / 80.0 * TAU).sin_cos();
                    let (x, z) = (c * 1.28, s * 1.28);
                    let (y1, z1) = (-z * sa, z * ca);
                    projeter([x * cb + z1 * sb, y1, -x * sb + z1 * cb], &v.camera).pos
                })
                .collect();
            // La normale du plan de l'anneau, tournée comme lui : presque perpendiculaire à la vue, il est de tranche.
            let normale = tourner([sa * sb, ca, sa * cb], v.camera.lacet, v.camera.tangage);
            let trait_ = Stroke::new(1.0, ANNEAU);
            if normale[2].abs() < 0.05 {
                for k in 0..points.len() {
                    p.line_segment([points[k], points[(k + 1) % points.len()]], trait_);
                }
            } else {
                p.add(Shape::closed_line(points, trait_));
            }
        }
    }

    /// Les synapses, de la couleur de leur lobe, plus vives devant et quand elles portent une lueur ; celles d'une
    /// place sans destination restent éteintes. Presque effacées quand le réseau dessine une image (cdc:365) ; une sur
    /// deux en mode économe (cdc:363).
    fn peindre_synapses(&self, p: &Painter, temps: f64) {
        let efface = 1.0 - self.morph * 0.85;
        for &(a, b) in &self.synapses {
            if self.econome && (a + b) % 2 == 1 {
                continue;
            }
            let (couleur, naissance) = match (self.pole_de[a], self.pole_de[b]) {
                (Some(k), _) | (_, Some(k)) => (self.poles[k].couleur, self.naissance(k, temps)),
                _ if a > NEURONES || b > NEURONES => continue,
                _ => (self.couleur_lobe(if a == 0 { b } else { a }, temps), 1.0),
            };
            let (pa, pb) = (self.projetes[a], self.projetes[b]);
            let devant = 1.0 - ((pa.z + pb.z) * 0.5 + 1.2) / 2.4;
            let force = (0.05 + 0.13 * devant + (self.lueur(a, temps) + self.lueur(b, temps)) * 0.35).min(0.9)
                * naissance
                * efface;
            if force > 0.01 {
                p.line_segment([pa.pos, pb.pos], Stroke::new(1.0, lumiere(couleur, force)));
            }
        }
    }

    fn peindre_ambiantes(&self, p: &Painter, v: &Vue, temps: f64) {
        for m in &self.ambiantes {
            let f = ((temps - m.depart) / m.duree).clamp(0.0, 1.0) as f32;
            let (a, b) = (self.projetes[m.de], self.projetes[m.vers]);
            let r = 1.6 * a.s * v.taille.sqrt();
            p.circle_filled(a.pos + (b.pos - a.pos) * f, r, lumiere(self.couleur_place(m.de, temps), 0.8));
        }
    }

    /// Les neurones : plus gros et plus vifs quand une lueur les traverse, avec un halo ; et quand ils dessinent une
    /// image, ou que leur personne est nommée (cdc:377-378).
    fn peindre_neurones(&self, p: &Painter, v: &Vue, temps: f64) {
        let taille = v.taille.sqrt();
        let teinte = self.teinte_a(temps);
        let mut forces = [0.0f32; MAX_PERSONNES];
        for (f, e) in forces.iter_mut().zip(&self.eclairages) {
            *f = e.force(temps);
        }
        let m = self.morph;
        for i in 1..=NEURONES {
            let q = self.projetes[i];
            let lueur = self.lueur(i, temps);
            let devant = 1.0 - (q.z + 1.2) / 2.4;
            let (c, personne) = self.couleur_neurone(i, temps, teinte, &forces);
            let r = (1.3 + lueur * 3.2 + m * 1.4 + personne * 1.2) * q.s * taille;
            p.circle_filled(q.pos, r, lumiere(c, 0.25 + 0.5 * devant + lueur + m * 0.5));
            if lueur > 0.25 {
                p.circle_filled(q.pos, r * 3.4, lumiere(c, lueur * 0.25));
            }
        }
    }

    /// Où en est une impulsion : la synapse qu'elle parcourt et la part déjà faite.
    fn position(&self, imp: &Impulsion, temps: f64) -> (usize, f32) {
        let avance = ((temps - imp.depart) * f64::from(imp.vitesse)).max(0.0) as f32;
        let pas = (avance.floor() as usize).min(imp.chemin.len() - 2);
        (pas, (avance - pas as f32).clamp(0.0, 1.0))
    }

    fn tete(&self, imp: &Impulsion, temps: f64) -> Pos2 {
        let (pas, f) = self.position(imp, temps);
        let (a, b) = (self.projetes[imp.chemin[pas]].pos, self.projetes[imp.chemin[pas + 1]].pos);
        a + (b - a) * f
    }

    /// Les impulsions : une tête lumineuse, et une traînée sur les trois synapses d'avant, qui s'éteint.
    fn peindre_impulsions(&self, p: &Painter, v: &Vue, temps: f64) {
        for imp in &self.impulsions {
            let (pas, f) = self.position(imp, temps);
            let avance = pas as f32 + f;
            for j in pas.saturating_sub(3)..=pas {
                let (a, b) = (self.projetes[imp.chemin[j]], self.projetes[imp.chemin[j + 1]]);
                let bout = if j == pas { f } else { 1.0 };
                let force = (1.0 - (avance - j as f32 - 0.5) / 3.5).clamp(0.0, 1.0) * 0.85;
                p.line_segment(
                    [a.pos, a.pos + (b.pos - a.pos) * bout],
                    Stroke::new(2.4 * a.s, lumiere(imp.couleur, force)),
                );
            }
            halo(p, self.tete(imp, temps), 16.0 * v.taille, imp.couleur, 1.0);
        }
    }

    /// Les destinations, du plus loin au plus proche, Eve à sa profondeur parmi elles : un disque sombre cerclé de leur
    /// couleur, qui bat quand un outil y travaille, éclate quand une impulsion arrive, se cercle de rouge quand un
    /// outil y échoue ; leur nombre d'actions au centre. Puis leurs étiquettes, par-dessus tout le réseau. Rend la
    /// place et le rayon de chacune (pour le survol), et le côté où chaque étiquette s'est posée.
    fn peindre_poles(&self, p: &Painter, v: &Vue, temps: f64) -> (Vec<(usize, Pos2, f32)>, Vec<(usize, u8)>) {
        let z = |k: usize| self.projetes[self.poles[k].place].z;
        let mut ordre: Vec<usize> = (0..self.poles.len()).collect();
        ordre.sort_by(|&a, &b| z(b).total_cmp(&z(a)));
        let mut reperes = Vec::with_capacity(ordre.len());
        let mut eve = false;
        for &k in &ordre {
            // Eve est au centre (profondeur nulle) : les destinations de devant la recouvrent.
            if !eve && z(k) < 0.0 {
                self.peindre_eve(p, v, temps);
                eve = true;
            }
            let naissance = self.naissance(k, temps);
            if naissance <= 0.0 {
                continue;
            }
            let pole = &self.poles[k];
            let q = self.projetes[pole.place];
            let loin = estompe(q.z);
            let eclat = (1.0 - (temps - pole.eclat) / DUREE_ECLAT).clamp(0.0, 1.0) as f32;
            let actif = pole.en_cours > 0;
            let battement = if actif { 3.0 * (temps * 6.0).sin() as f32 } else { 0.0 };
            let r = (14.0 + battement + eclat * 8.0) * q.s * v.taille * naissance;
            let force = (0.35 + eclat * 0.9 + if actif { 0.5 } else { 0.0 }) * loin;
            for (f, a) in [(2.2, 0.05), (1.65, 0.08), (1.28, 0.12)] {
                p.circle_filled(q.pos, r * f, lumiere(pole.couleur, a * force));
            }
            let cerne = Stroke::new(if actif { 3.0 } else { 1.8 }, voile(pole.couleur, loin));
            p.circle(q.pos, r, voile(NUIT, loin), cerne);
            if actif {
                let onde = r + (8.0 + ((temps * 30.0) % 14.0) as f32) * v.taille;
                p.circle_stroke(q.pos, onde, Stroke::new(1.0, voile(pole.couleur, 0.5 * loin)));
            }
            let depuis = temps - pole.erreur;
            if depuis < DUREE_ERREUR {
                let s = (temps * 5.0).sin() as f32;
                let fondu = 1.0 - (depuis / DUREE_ERREUR) as f32;
                let rouge = voile(ROUGE, loin * (0.5 + 0.45 * s) * fondu);
                p.circle_stroke(q.pos, r + (7.0 + 5.0 * s) * v.taille, Stroke::new(2.4, rouge));
            }
            if r >= 8.0 {
                let taille = (r * 0.95).clamp(8.0, 16.0);
                ecrire(p, q.pos, Align2::CENTER_CENTER, &pole.total.to_string(), taille, voile(pole.couleur, loin));
            }
            reperes.push((k, q.pos, r));
        }
        if !eve {
            self.peindre_eve(p, v, temps);
        }
        // Les étiquettes : d'abord celles des destinations qui travaillent ou viennent de recevoir, puis des plus
        // proches. Chacune se pose sous sa destination, au-dessus, à droite ou à gauche, à la première place libre (sa
        // place d'avant d'abord : elle ne saute pas d'une image à l'autre), hors des bandeaux du haut et du bas ; sans
        // place libre, elle attend que le réseau ait tourné.
        let vive = |k: usize| {
            let q = &self.poles[k];
            q.en_cours > 0 || temps - q.eclat < DUREE_ECLAT || temps - q.erreur < DUREE_ERREUR
        };
        let mut priorite: Vec<(usize, f32)> = reperes.iter().map(|&(k, _, r)| (k, r)).collect();
        priorite.sort_by(|a, b| vive(b.0).cmp(&vive(a.0)).then(z(a.0).total_cmp(&z(b.0))));
        let foule = self.poles.len() > FOULE;
        let (haut, bas) = (v.rect.top() + BANDEAU, v.rect.bottom() - BANDEAU);
        let mut posees: Vec<Rect> = Vec::new();
        let mut cotes = Vec::new();
        for (k, r) in priorite {
            let pole = &self.poles[k];
            let q = self.projetes[pole.place];
            if !vive(k) && foule && q.z > 0.45 {
                continue;
            }
            let taille = (11.0 * q.s).clamp(10.0, 13.0).round();
            let galley = p.layout_no_wrap(pole.nom.clone(), FontId::monospace(taille), Color32::PLACEHOLDER);
            let (g, marge, ecart) = (galley.size(), vec2(4.0, 1.0), r + 5.0 * v.taille);
            let place = |cote: u8| {
                let c = match cote {
                    0 => Align2::CENTER_TOP.anchor_size(q.pos + vec2(0.0, ecart), g),
                    1 => Align2::CENTER_BOTTOM.anchor_size(q.pos - vec2(0.0, ecart), g),
                    2 => Align2::LEFT_CENTER.anchor_size(q.pos + vec2(ecart + 3.0, 0.0), g),
                    _ => Align2::RIGHT_CENTER.anchor_size(q.pos - vec2(ecart + 3.0, 0.0), g),
                }
                .expand2(marge);
                let dx = (v.rect.left() + 2.0 - c.left()).max(0.0) + (v.rect.right() - 2.0 - c.right()).min(0.0);
                c.translate(vec2(dx, 0.0))
            };
            let libre = |c: &Rect| c.top() >= haut && c.bottom() <= bas && !posees.iter().any(|x| x.intersects(*c));
            let Some((cote, cadre)) =
                std::iter::once(pole.cote).chain(0..4).map(|c| (c, place(c))).find(|(_, c)| libre(c))
            else {
                continue;
            };
            let loin = estompe(q.z);
            p.rect_filled(cadre, 3.0, voile(ENCRE, 0.72 * loin));
            let couleur = if pole.en_cours > 0 { voile([255, 255, 255], loin.max(0.85)) } else { voile(PALE, loin) };
            p.galley(cadre.min + marge, galley, couleur);
            posees.push(cadre);
            cotes.push((k, cote));
        }
        (reperes, cotes)
    }

    /// Le mot que portent les impulsions les plus récentes (le fichier, le site, ERREUR), dans un cartouche qui les
    /// suit ; six au plus, sans chevauchement, quand Claude enchaîne les outils.
    fn peindre_etiquettes_des_impulsions(&self, p: &Painter, temps: f64) {
        let mut posees: Vec<Rect> = Vec::new();
        for imp in self.impulsions.iter().rev().filter(|i| !i.etiquette.is_empty()) {
            let tete = self.tete(imp, temps);
            let galley = p.layout_no_wrap(imp.etiquette.clone(), FontId::monospace(11.0), Color32::PLACEHOLDER);
            let cadre = Rect::from_min_size(tete + vec2(12.0, -20.0), galley.size() + vec2(10.0, 4.0));
            if posees.iter().any(|x| x.intersects(cadre)) {
                continue;
            }
            p.rect_filled(cadre, 2.0, voile(ENCRE, 0.78));
            p.galley(cadre.min + vec2(5.0, 2.0), galley, voile(imp.couleur, 1.0));
            posees.push(cadre);
            if posees.len() == 6 {
                break;
            }
        }
    }

    /// Eve, au cœur : un disque sombre cerclé de cyan dans son halo, qui respire, grossit quand un outil tourne, quand
    /// on lui parle, et s'éclaire quand une réponse lui revient.
    fn peindre_eve(&self, p: &Painter, v: &Vue, temps: f64) {
        if !self.montrer_eve {
            return;
        }
        let q = self.projetes[0];
        let eclat = (1.0 - (temps - self.eclat_eve) / DUREE_ECLAT).clamp(0.0, 1.0) as f32;
        let actif = !self.en_cours.is_empty();
        let souffle = (temps * 2.2).sin() as f32 * 2.0;
        let r = (30.0 + souffle + eclat * 8.0 + if actif { 4.0 } else { 0.0 } + self.voix * 10.0) * q.s * v.taille;
        halo(p, q.pos, r * 2.8, CYAN, 0.55 + eclat * 0.3);
        for (f, a) in [(1.35, 0.06), (1.15, 0.1)] {
            p.circle_filled(q.pos, r * f, lumiere(CYAN, a));
        }
        p.circle(q.pos, r, NUIT_EVE, Stroke::new(2.4, voile(CYAN, 1.0)));
        let grand = (20.0 * q.s * v.taille).clamp(12.0, 24.0);
        ecrire(p, q.pos - vec2(0.0, 3.0 * v.taille), Align2::CENTER_CENTER, "EVE", grand, voile(BLANC, 1.0));
        let (mot, couleur) = if actif { ("TRAITEMENT", VERT) } else { ("PRÊTE", GRIS) };
        let petit = (10.0 * v.taille.sqrt()).clamp(9.0, 11.0);
        ecrire(p, q.pos + vec2(0.0, 14.0 * v.taille), Align2::CENTER_CENTER, mot, petit, voile(couleur, 1.0));
    }

    /// Les ondes et les étincelles des arrivées ; elles suivent leur point quand le réseau tourne.
    fn peindre_eclats(&self, p: &Painter, v: &Vue, temps: f64) {
        for o in &self.ondes {
            let age = ((temps - o.depart) / DUREE_ONDE).clamp(0.0, 1.0) as f32;
            let r = (6.0 + 84.0 * age) * v.taille;
            p.circle_stroke(self.projetes[o.place].pos, r, Stroke::new(2.0, lumiere(o.couleur, 1.0 - age)));
        }
        for e in &self.etincelles {
            let age = (temps - e.depart).max(0.0) as f32;
            let vie = 1.0 - age / DUREE_ETINCELLE as f32;
            if vie > 0.0 {
                // L'élan du modèle (0,96 par image) : la course tend vers vitesse / 1,22.
                let d = e.vitesse / 1.2246 * (1.0 - (-1.2246 * age).exp()) * v.taille;
                p.circle_filled(self.projetes[e.place].pos + e.direction * d, 2.0, lumiere(e.couleur, vie));
            }
        }
    }

    /// Le cadre : le compte des actions en haut à gauche, la légende en haut à droite (s'il y a la place), la dernière
    /// action en toutes lettres au bas, qui s'efface après dix secondes.
    fn peindre_cadre(&self, p: &Painter, v: &Vue, temps: f64) {
        let r = v.rect;
        let gros = |c: Rgb| TextFormat::simple(FontId::monospace(15.0), voile(c, 1.0));
        let petit = || TextFormat::simple(FontId::monospace(10.0), voile(GRIS, 1.0));
        let mut job = LayoutJob::default();
        job.append(&self.total.to_string(), 0.0, gros(CYAN));
        job.append(if self.total > 1 { " ACTIONS" } else { " ACTION" }, 0.0, petit());
        job.append(&self.en_cours.len().to_string(), 14.0, gros(VERT));
        job.append(" EN COURS", 0.0, petit());
        p.galley(r.min + vec2(10.0, 8.0), p.layout_job(job), Color32::PLACEHOLDER);
        if r.width() >= 420.0 {
            let (mut x, y) = (r.right() - 10.0, r.top() + 17.0);
            for (mot, c) in [("ERREUR", ROUGE), ("RÉPONSE", VERT), ("REQUÊTE", CYAN)] {
                x -= ecrire(p, pos2(x, y), Align2::RIGHT_CENTER, mot, 10.0, voile(GRIS, 1.0)).width() + 9.0;
                p.circle_filled(pos2(x, y), 7.0, lumiere(c, 0.25));
                p.circle_filled(pos2(x, y), 3.5, voile(c, 1.0));
                x -= 16.0;
            }
        }
        if let Some(d) = &self.derniere {
            let age = temps - d.heure;
            let force = (1.0 - (age - 10.0) / 4.0).clamp(0.0, 1.0) as f32;
            if force > 0.0 {
                let pole = &self.poles[d.pole];
                let police = || FontId::monospace(12.0);
                let mut job = LayoutJob::default();
                job.append(&d.texte, 0.0, TextFormat::simple(police(), voile(BLANC, force)));
                job.append("  →  ", 0.0, TextFormat::simple(police(), voile(GRIS, force)));
                job.append(&pole.nom, 0.0, TextFormat::simple(police(), voile(pole.couleur, force)));
                job.wrap = TextWrapping::truncate_at_width(r.width() - 24.0);
                let g = p.layout_job(job);
                let coin = pos2(r.center().x - g.size().x / 2.0, r.bottom() - 10.0 - g.size().y);
                p.galley(coin, g, Color32::PLACEHOLDER);
            }
        }
    }

    /// La bulle de la destination sous la souris : son nom, ses actions, la dernière.
    fn peindre_bulle(&self, p: &Painter, v: &Vue, reperes: &[(usize, Pos2, f32)], souris: Pos2) {
        // Les repères vont du plus loin au plus proche : la bulle est celle de la plus proche sous la souris.
        let Some(&(k, _, _)) = reperes.iter().rev().find(|(_, c, r)| c.distance(souris) <= r + 8.0) else { return };
        let pole = &self.poles[k];
        let s = |n: u32| if n > 1 { "s" } else { "" };
        let mut compte = format!("\n{} action{} · {} en cours", pole.total, s(pole.total), pole.en_cours);
        if pole.erreurs > 0 {
            compte += &format!(" · {} erreur{}", pole.erreurs, s(pole.erreurs));
        }
        let derniere = if pole.derniere.is_empty() {
            "\naucune action pour l'instant".to_string()
        } else {
            format!("\ndernière : {}", pole.derniere)
        };
        let mut job = LayoutJob::default();
        job.append(&pole.nom, 0.0, TextFormat::simple(FontId::monospace(12.0), voile(pole.couleur, 1.0)));
        job.append(&compte, 0.0, TextFormat::simple(FontId::monospace(11.0), voile(BLANC, 1.0)));
        job.append(&derniere, 0.0, TextFormat::simple(FontId::monospace(11.0), voile(PALE, 1.0)));
        job.wrap = TextWrapping { max_width: 300.0, max_rows: 7, break_anywhere: true, ..Default::default() };
        let g = p.layout_job(job);
        let taille = g.size() + vec2(16.0, 12.0);
        let x = (souris.x + 16.0).min(v.rect.right() - taille.x - 4.0).max(v.rect.left() + 4.0);
        let y = (souris.y + 16.0).min(v.rect.bottom() - taille.y - 4.0).max(v.rect.top() + 4.0);
        let cadre = Rect::from_min_size(pos2(x, y), taille);
        p.rect(cadre, 3.0, voile([4, 12, 24], 0.94), Stroke::new(1.0, voile(pole.couleur, 0.9)), StrokeKind::Inside);
        p.galley(cadre.min + vec2(8.0, 6.0), g, Color32::PLACEHOLDER);
    }
}

/// Le cerveau, tiré une fois pour toutes : ses points, ses synapses, l'ordre des places des destinations.
struct Cerveau {
    places: Vec<[f32; 3]>,
    voisins: Vec<Vec<usize>>,
    synapses: Vec<(usize, usize)>,
    ordre: Vec<usize>,
}

/// Le cerveau du modèle : Eve au centre ; 330 neurones tirés dans une boule, sept sur dix près de sa surface ; chacun
/// lié à ses quatre plus proches voisins, Eve à ses douze, chaque place de destination à ses huit ; puis tout neurone
/// isolé rattaché au plus proche de ceux qu'Eve atteint. Les 60 places des destinations sont posées en spirale (le
/// nombre d'or) sur une sphère un peu plus grande.
fn construire() -> Cerveau {
    let mut h = Hasard(GRAINE);
    let mut places = vec![[0.0f32; 3]];
    for _ in 0..NEURONES {
        let d = loop {
            let (x, y, z) = (h.suivant() * 2.0 - 1.0, h.suivant() * 2.0 - 1.0, h.suivant() * 2.0 - 1.0);
            let l = x * x + y * y + z * z;
            if (0.01..=1.0).contains(&l) {
                let l = l.sqrt();
                break [x / l, y / l, z / l];
            }
        };
        let r = if h.suivant() < 0.7 { 0.78 + h.suivant() * 0.24 } else { 0.22 + h.suivant() * 0.5 };
        places.push([(d[0] * r) as f32, (d[1] * r) as f32, (d[2] * r) as f32]);
    }
    for k in 0..MAX_DESTINATIONS {
        let y = 1.0 - 2.0 * (k as f32 + 0.5) / MAX_DESTINATIONS as f32;
        let rr = (1.0 - y * y).sqrt();
        let (s, c) = (k as f32 * 2.399_963).sin_cos();
        places.push([c * rr * RAYON_DESTINATIONS, y * RAYON_DESTINATIONS, s * rr * RAYON_DESTINATIONS]);
    }
    let mut voisins = vec![Vec::new(); places.len()];
    let mut synapses = Vec::new();
    let neurone = |j: usize| (1..=NEURONES).contains(&j);
    for i in 1..=NEURONES {
        for j in proches(&places, i, 4, neurone) {
            lier(&mut voisins, &mut synapses, i, j);
        }
    }
    for k in 0..MAX_DESTINATIONS {
        let place = 1 + NEURONES + k;
        for j in proches(&places, place, 8, neurone) {
            lier(&mut voisins, &mut synapses, place, j);
        }
    }
    for j in proches(&places, 0, 12, neurone) {
        lier(&mut voisins, &mut synapses, 0, j);
    }
    loop {
        let vus = atteints(&voisins);
        let Some(u) = (1..=NEURONES).find(|&u| !vus[u]) else { break };
        let b = (1..=NEURONES)
            .filter(|&j| vus[j])
            .min_by(|&a, &b| d2(places[u], places[a]).total_cmp(&d2(places[u], places[b])))
            .unwrap_or(0);
        lier(&mut voisins, &mut synapses, u, b);
    }
    // L'ordre des places : d'abord la plus près de l'équateur, puis chaque fois la plus loin de celles déjà données.
    // Les premières destinations, les plus fréquentes, se répartissent ainsi sur toute la sphère.
    let place = |k: usize| places[1 + NEURONES + k];
    let premiere = (0..MAX_DESTINATIONS).min_by(|&a, &b| place(a)[1].abs().total_cmp(&place(b)[1].abs())).unwrap_or(0);
    let mut donnees = vec![premiere];
    let mut ecart: Vec<f32> = (0..MAX_DESTINATIONS).map(|k| d2(place(k), place(premiere))).collect();
    while donnees.len() < MAX_DESTINATIONS {
        let suivante = (0..MAX_DESTINATIONS)
            .filter(|k| !donnees.contains(k))
            .max_by(|&a, &b| ecart[a].total_cmp(&ecart[b]).then(b.cmp(&a)))
            .unwrap_or(0);
        donnees.push(suivante);
        for k in 0..MAX_DESTINATIONS {
            ecart[k] = ecart[k].min(d2(place(k), place(suivante)));
        }
    }
    let ordre = donnees.into_iter().map(|k| 1 + NEURONES + k).collect();
    Cerveau { places, voisins, synapses, ordre }
}

/// Les `k` points retenus les plus proches du point `i` (à distance égale, le plus petit numéro d'abord).
fn proches(places: &[[f32; 3]], i: usize, k: usize, retenir: impl Fn(usize) -> bool) -> Vec<usize> {
    let mut d: Vec<(f32, usize)> =
        (0..places.len()).filter(|&j| j != i && retenir(j)).map(|j| (d2(places[i], places[j]), j)).collect();
    d.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    d.into_iter().take(k).map(|(_, j)| j).collect()
}

fn lier(voisins: &mut [Vec<usize>], synapses: &mut Vec<(usize, usize)>, a: usize, b: usize) {
    if a != b && !voisins[a].contains(&b) {
        voisins[a].push(b);
        voisins[b].push(a);
        synapses.push((a.min(b), a.max(b)));
    }
}

/// Les points qu'Eve atteint par les neurones seuls.
fn atteints(voisins: &[Vec<usize>]) -> Vec<bool> {
    let mut vus = vec![false; voisins.len()];
    vus[0] = true;
    let mut pile = vec![0];
    while let Some(u) = pile.pop() {
        for &v in &voisins[u] {
            if v <= NEURONES && !vus[v] {
                vus[v] = true;
                pile.push(v);
            }
        }
    }
    vus
}

fn d2(a: [f32; 3], b: [f32; 3]) -> f32 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)
}

/// Le tirage du modèle (mulberry32), au bit près : la même graine donne le même cerveau que sa page.
struct Hasard(u32);

impl Hasard {
    /// Dans [0, 1[.
    fn suivant(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x6D2B_79F5);
        let a = self.0;
        let mut t = (a ^ (a >> 15)).wrapping_mul(1 | a);
        t = t.wrapping_add((t ^ (t >> 7)).wrapping_mul(61 | t)) ^ t;
        f64::from(t ^ (t >> 14)) / 4_294_967_296.0
    }

    fn f(&mut self) -> f32 {
        self.suivant() as f32
    }

    fn entre(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.f()
    }

    /// Dans 0..n.
    fn indice(&mut self, n: usize) -> usize {
        ((self.suivant() * n as f64) as usize).min(n.saturating_sub(1))
    }
}

/// La caméra : la rotation autour de l'axe vertical (le lacet), puis l'inclinaison (le tangage), et la perspective d'un
/// œil à `RECUL` rayons du centre. L'axe y de l'écran descend, comme celui du modèle.
#[derive(Clone, Copy, Debug)]
struct Camera {
    lacet: f32,
    tangage: f32,
    centre: Pos2,
    /// Les points par unité du réseau, à la profondeur du centre.
    echelle: f32,
}

/// Un point projeté : sa place à l'écran, sa profondeur après rotation (négative devant, vers l'œil), et son facteur de
/// taille (plus de 1 devant, moins de 1 derrière).
#[derive(Clone, Copy, Debug, Default)]
struct Projete {
    pos: Pos2,
    z: f32,
    s: f32,
}

/// Tourne un point du réseau : autour de l'axe vertical, puis autour de l'axe horizontal de l'écran (les formules du
/// modèle).
fn tourner(p: [f32; 3], lacet: f32, tangage: f32) -> [f32; 3] {
    let ((sl, cl), (st, ct)) = (lacet.sin_cos(), tangage.sin_cos());
    let x = p[0] * cl - p[2] * sl;
    let z0 = p[0] * sl + p[2] * cl;
    [x, p[1] * ct - z0 * st, p[1] * st + z0 * ct]
}

/// La perspective : un point plus près de l'œil est plus grand et plus loin du centre.
fn projeter(p: [f32; 3], c: &Camera) -> Projete {
    let [x, y, z] = tourner(p, c.lacet, c.tangage);
    let s = RECUL / (RECUL + z).max(0.1);
    let k = c.echelle * s;
    Projete { pos: pos2(c.centre.x + x * k, c.centre.y + y * k), z, s }
}

/// L'opacité d'une destination selon sa profondeur : celles de derrière pâlissent (le modèle).
fn estompe(z: f32) -> f32 {
    (1.0 - z.max(0.0) * 0.55).max(0.35)
}

/// Une couleur qui s'ajoute à ce qu'elle recouvre (le `lighter` du canevas du modèle) : les lueurs s'additionnent.
fn lumiere(c: Rgb, force: f32) -> Color32 {
    let f = force.clamp(0.0, 1.0);
    Color32::from_rgb_additive((f32::from(c[0]) * f) as u8, (f32::from(c[1]) * f) as u8, (f32::from(c[2]) * f) as u8)
}

/// Une couleur ordinaire, plus ou moins transparente.
fn voile(c: Rgb, opacite: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], (opacite.clamp(0.0, 1.0) * 255.0) as u8)
}

fn melange(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let m = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    [m(a[0], b[0]), m(a[1], b[1]), m(a[2], b[2])]
}

/// Une lueur ronde, plus vive au centre : des disques additifs empilés, faute du dégradé radial du canevas.
fn halo(p: &Painter, centre: Pos2, rayon: f32, couleur: Rgb, force: f32) {
    for (f, a) in [(1.0, 0.10), (0.68, 0.16), (0.42, 0.28), (0.2, 0.6)] {
        p.circle_filled(centre, rayon * f, lumiere(couleur, a * force));
    }
}

/// Un texte d'une ligne, posé selon `ancre`. La taille s'arrondit au point et la couleur s'applique après la mise en
/// page : egui garde ainsi ses glyphes et ses mises en page d'une image à l'autre, même quand une étiquette grandit en
/// tournant ou pâlit au loin.
fn ecrire(p: &Painter, pos: Pos2, ancre: Align2, texte: &str, taille: f32, couleur: Color32) -> Rect {
    let galley = p.layout_no_wrap(texte.to_string(), FontId::monospace(taille.round()), Color32::PLACEHOLDER);
    let rect = ancre.anchor_size(pos, galley.size());
    p.galley(rect.min, galley, couleur);
    rect
}

// ---- Le sujet : les images au trait, et ce qui y fait penser ------------------------------------------------------

/// Le nom d'une image connue, tel que le réseau le garde.
fn image_connue(nom: &str) -> Option<&'static str> {
    let nom = nom.trim();
    IMAGES.iter().find(|(n, _)| *n == nom).map(|(n, _)| *n)
}

/// Son nom dans la ligne d'état (« ÉCOLE », « ARGENT »).
fn nom_d_image(nom: &str) -> &'static str {
    IMAGES.iter().find(|(n, _)| *n == nom).map_or("", |(_, m)| *m)
}

/// L'arrivée douce d'un dessin (cdc:219).
fn lisse(m: f32) -> f32 {
    let m = m.clamp(0.0, 1.0);
    m * m * (3.0 - 2.0 * m)
}

/// Les traits d'une image, en coordonnées de −1 à 1, y vers le bas : ceux du modèle, au chiffre près (cdc:161-184).
/// « avions » a ceux d'« avion » : il en fait deux, qui volent ([`avions`]).
fn traits(nom: &str) -> Vec<Vec<[f64; 2]>> {
    // Les cercles du modèle : n + 1 points, et son 6,2832 pour un tour (pas 2π : les mêmes points que sa page).
    fn cercle(cx: f64, cy: f64, r: f64, n: usize) -> Vec<[f64; 2]> {
        (0..=n)
            .map(|k| {
                let a = k as f64 / n as f64 * 6.2832;
                [cx + a.cos() * r, cy + a.sin() * r]
            })
            .collect()
    }
    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<[f64; 2]> {
        vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1], [x0, y0]]
    }
    match nom {
        "ecole" => vec![
            rect(-0.8, -0.1, 0.8, 0.65),
            vec![[-0.92, -0.1], [0.0, -0.62], [0.92, -0.1]],
            vec![[0.0, -0.62], [0.0, -0.98], [0.32, -0.88], [0.0, -0.78]],
            vec![[-0.15, 0.65], [-0.15, 0.25], [0.15, 0.25], [0.15, 0.65]],
            rect(-0.62, 0.02, -0.36, 0.3),
            rect(0.36, 0.02, 0.62, 0.3),
            cercle(0.0, -0.3, 0.1, 16),
        ],
        // Une pièce de monnaie, son dollar.
        "piece" => vec![
            cercle(0.0, 0.0, 0.88, 48),
            cercle(0.0, 0.0, 0.72, 48),
            vec![[0.0, -0.62], [0.0, 0.62]],
            vec![
                [0.3, -0.3],
                [0.2, -0.45],
                [0.0, -0.5],
                [-0.2, -0.45],
                [-0.3, -0.3],
                [-0.2, -0.12],
                [0.0, 0.0],
                [0.2, 0.12],
                [0.3, 0.3],
                [0.2, 0.45],
                [0.0, 0.5],
                [-0.2, 0.45],
                [-0.3, 0.3],
            ],
        ],
        // Une enveloppe.
        "lettre" => vec![
            rect(-0.92, -0.55, 0.92, 0.55),
            vec![[-0.92, -0.55], [0.0, 0.1], [0.92, -0.55]],
            vec![[-0.92, 0.55], [-0.35, 0.0]],
            vec![[0.92, 0.55], [0.35, 0.0]],
        ],
        "agenda" => vec![
            rect(-0.8, -0.7, 0.8, 0.85),
            vec![[-0.8, -0.3], [0.8, -0.3]],
            vec![[-0.4, -0.9], [-0.4, -0.55]],
            vec![[0.4, -0.9], [0.4, -0.55]],
            vec![[-0.27, -0.3], [-0.27, 0.85]],
            vec![[0.27, -0.3], [0.27, 0.85]],
            vec![[-0.8, 0.1], [0.8, 0.1]],
            vec![[-0.8, 0.48], [0.8, 0.48]],
        ],
        "croix" => vec![vec![
            [-0.25, -0.85],
            [0.25, -0.85],
            [0.25, -0.25],
            [0.85, -0.25],
            [0.85, 0.25],
            [0.25, 0.25],
            [0.25, 0.85],
            [-0.25, 0.85],
            [-0.25, 0.25],
            [-0.85, 0.25],
            [-0.85, -0.25],
            [-0.25, -0.25],
            [-0.25, -0.85],
        ]],
        "globe" => vec![
            cercle(0.0, 0.0, 0.88, 48),
            // Le méridien : le même cercle aplati (et son y repris à 88 %, comme le modèle).
            cercle(0.0, 0.0, 0.88, 48).into_iter().map(|p| [p[0] * 0.42, p[1] * 0.88]).collect(),
            vec![[-0.88, 0.0], [0.88, 0.0]],
            vec![[-0.76, -0.44], [0.76, -0.44]],
            vec![[-0.76, 0.44], [0.76, 0.44]],
            vec![[0.0, -0.88], [0.0, 0.88]],
        ],
        // Une assiette, sa fourchette et son couteau.
        "repas" => vec![
            cercle(0.0, 0.0, 0.85, 48),
            cercle(0.0, 0.0, 0.55, 48),
            vec![[-1.0, -0.7], [-1.0, 0.7]],
            vec![[1.0, -0.7], [1.0, 0.7]],
        ],
        "maison" => vec![
            rect(-0.65, -0.05, 0.65, 0.8),
            vec![[-0.8, -0.05], [0.0, -0.65], [0.8, -0.05]],
            rect(0.3, -0.55, 0.5, -0.3),
            vec![[-0.12, 0.8], [-0.12, 0.35], [0.12, 0.35], [0.12, 0.8]],
            rect(-0.5, 0.08, -0.22, 0.32),
            rect(0.22, 0.08, 0.5, 0.32),
            vec![[-0.36, 0.08], [-0.36, 0.32]],
            vec![[-0.5, 0.2], [-0.22, 0.2]],
            cercle(0.08, 0.58, 0.02, 6),
            vec![[0.4, -0.6], [0.45, -0.75], [0.38, -0.88], [0.46, -1.0]],
        ],
        "avion" | "avions" => {
            let mut t = vec![
                vec![
                    [-0.92, -0.02],
                    [-0.78, -0.17],
                    [0.25, -0.2],
                    [0.65, -0.16],
                    [0.95, 0.02],
                    [0.68, 0.15],
                    [-0.7, 0.15],
                    [-0.92, -0.02],
                ],
                vec![[-0.78, -0.17], [-0.9, -0.62], [-0.68, -0.62], [-0.42, -0.19]],
                vec![[-0.02, 0.12], [-0.38, 0.62], [-0.14, 0.62], [0.34, 0.12]],
                vec![[-0.02, -0.17], [-0.3, -0.55], [-0.14, -0.55], [0.2, -0.19]],
                rect(0.02, 0.2, 0.22, 0.32),
                cercle(0.74, -0.04, 0.035, 8),
            ];
            // Les hublots.
            t.extend([-0.45, -0.3, -0.15, 0.0, 0.15, 0.3, 0.45].map(|x| cercle(x, -0.05, 0.03, 8)));
            t.push(vec![[-0.2, 0.15], [-0.2, 0.2]]);
            t.push(vec![[0.9, 0.08], [0.98, 0.08]]);
            t
        }
        // Un soleil qui sourit : seize rayons, longs et courts.
        "soleil" => {
            let mut t = vec![cercle(0.0, 0.0, 0.5, 40)];
            t.extend((0..16).map(|k| {
                let a = f64::from(k) / 16.0 * 6.2832;
                let (r0, r1) = if k % 2 == 1 { (0.66, 0.82) } else { (0.62, 0.98) };
                vec![[a.cos() * r0, a.sin() * r0], [a.cos() * r1, a.sin() * r1]]
            }));
            t.push(
                (0..13)
                    .map(|k| {
                        let a = 0.25 * 3.1416 + f64::from(k) / 12.0 * 0.5 * 3.1416;
                        [a.cos() * 0.3, a.sin() * 0.3]
                    })
                    .collect(),
            );
            t.push(cercle(-0.17, -0.12, 0.05, 10));
            t.push(cercle(0.17, -0.12, 0.05, 10));
            t
        }
        _ => Vec::new(),
    }
}

/// La place de chaque neurone sur une image (cdc:197-207) : autant de points que de neurones, à égale distance le long
/// des traits ; le neurone le plus à gauche prend le point le plus à gauche, et ainsi de suite (des trajets courts).
/// Rangée par numéro de point du réseau (la place 0, Eve, ne sert pas).
fn cibles(nom: &str, places: &[[f32; 3]]) -> Vec<[f32; 2]> {
    let mut sortie = vec![[0.0f32; 2]; NEURONES + 1];
    let segments: Vec<([f64; 2], [f64; 2], f64)> = traits(nom)
        .iter()
        .flat_map(|t| t.windows(2).map(|w| (w[0], w[1], (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]))))
        .collect();
    if segments.is_empty() || places.len() <= NEURONES {
        return sortie;
    }
    let longueur: f64 = segments.iter().map(|s| s.2).sum();
    let mut points: Vec<[f64; 2]> = (0..NEURONES)
        .map(|k| {
            // Le pas à pas du modèle, depuis le premier segment à chaque point : ses arrondis, donc ses points.
            let mut u = (k as f64 + 0.5) / NEURONES as f64 * longueur;
            let mut j = 0;
            while j < segments.len() - 1 && u > segments[j].2 {
                u -= segments[j].2;
                j += 1;
            }
            let (a, b, d) = segments[j];
            let f = (u / if d == 0.0 { 1.0 } else { d }).min(1.0);
            [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f]
        })
        .collect();
    let rang = |a: [f64; 2], b: [f64; 2]| {
        a[0].partial_cmp(&b[0]).unwrap_or(Ordering::Equal).then(a[1].partial_cmp(&b[1]).unwrap_or(Ordering::Equal))
    };
    points.sort_by(|a, b| rang(*a, *b));
    let base = |i: usize| [f64::from(places[i][0]), f64::from(places[i][1])];
    let mut neurones: Vec<usize> = (1..=NEURONES).collect();
    neurones.sort_by(|&i, &j| rang(base(i), base(j)));
    for (point, &i) in points.iter().zip(&neurones) {
        sortie[i] = [point[0] as f32, point[1] as f32];
    }
    sortie
}

/// Deux avions de neurones qui se croisent en volant (cdc:210-213) : les neurones pairs font le premier, qui va vers la
/// droite en haut ; les impairs le second, retourné, qui va vers la gauche en bas. Chacun traverse en 8,75 s.
fn avions(c: [f32; 2], i: usize, temps: f64) -> [f32; 2] {
    let premier = i % 2 == 0;
    let d = ((temps * 0.32 + if premier { 0.0 } else { 1.3 }).rem_euclid(2.8) - 1.4) as f32;
    let roulis = ((temps * 2.1 + if premier { 0.0 } else { 2.0 }).sin() * 0.06) as f32;
    if premier { [c[0] * 0.5 + d, c[1] * 0.5 - 0.38 + roulis] } else { [-c[0] * 0.5 - d, c[1] * 0.5 + 0.38 - roulis] }
}

/// Le quartier d'un point quand il y a `n` personnes : la sphère coupée en `n` parts égales autour de l'axe vertical ;
/// à quatre, les quartiers du modèle, par le signe de x et de z (cdc:532).
fn secteur(x: f32, z: f32, n: usize) -> usize {
    match n {
        0 | 1 => 0,
        4 => usize::from(x >= 0.0) + 2 * usize::from(z >= 0.0),
        _ => (((z.atan2(x) + PI) / TAU * n as f32) as usize).min(n - 1),
    }
}

/// Un texte prêt à y chercher des mots : en minuscules, sans accents (« Pédago » vaut « pedago »).
fn plier(texte: &str) -> String {
    let mut s = String::with_capacity(texte.len());
    for c in texte.chars().flat_map(char::to_lowercase) {
        match c {
            'à' | 'â' | 'ä' | 'á' | 'ã' | 'å' => s.push('a'),
            'ç' => s.push('c'),
            'é' | 'è' | 'ê' | 'ë' => s.push('e'),
            'î' | 'ï' | 'í' | 'ì' => s.push('i'),
            'ô' | 'ö' | 'ó' | 'ò' | 'õ' => s.push('o'),
            'ù' | 'û' | 'ü' | 'ú' => s.push('u'),
            'ÿ' | 'ý' => s.push('y'),
            'ñ' => s.push('n'),
            'œ' => s.push_str("oe"),
            'æ' => s.push_str("ae"),
            c => s.push(c),
        }
    }
    s
}

/// `motif` paraît dans `texte` en mots entiers (tous deux pliés) : « lea » dans « lea.md » ou « horaire de lea », pas
/// dans « clea » ni « leandre ».
fn en_mots_entiers(texte: &str, motif: &str) -> bool {
    let lettre = |c: Option<char>| c.is_some_and(char::is_alphanumeric);
    !motif.is_empty()
        && texte
            .match_indices(motif)
            .any(|(i, m)| !lettre(texte[..i].chars().next_back()) && !lettre(texte[i + m.len()..].chars().next()))
}

/// Les mots du modèle qui font penser à une image (cdc:188-193), dans son ordre, pliés. Ses prénoms et le groupe de
/// l'école des enfants n'y sont plus : la fenêtre les ajoute ([`Reseau::evoquer`]).
const MOTS_DES_IMAGES: [(&str, &[&str]); 6] = [
    ("ecole", &["ecole", "garderie", "pedago", "maternelle", "enseignant"]),
    (
        "piece",
        &[
            "factur", "compta", "impot", "taxe", "budget", "paiement", "allocation", "hydro", "koodo", "videotron",
            "reee", "celi", "reer",
        ],
    ),
    ("croix", &["sante", "dentiste", "vaccin", "811", "clsc", "medecin", "fievre"]),
    ("repas", &["menu", "repas", "epicerie", "souper", "lunch", "recette"]),
    ("agenda", &["agenda", ".ics", "calendrier", "rendez", "resume", "semaine", "horaire"]),
    ("lettre", &["courriel", "gmail", "brouillon", ".eml", "mail"]),
];

/// L'image à laquelle fait penser un texte plié (`sujetDe`, cdc:186-195) : d'abord les mots qu'a ajoutés la fenêtre
/// (en mots entiers), puis ceux du modèle (n'importe où, comme lui : « factur » dans « factures »), dans l'ordre ; à
/// défaut, la destination de l'outil : les courriels d'Outlook, le Web.
fn sujet_de(
    plie: &str,
    evocations: &[(&'static str, Vec<String>)],
    destination: Option<&Destination>,
) -> Option<&'static str> {
    if let Some((image, _)) = evocations.iter().find(|(_, mots)| mots.iter().any(|m| en_mots_entiers(plie, m))) {
        return Some(image);
    }
    if let Some((image, _)) = MOTS_DES_IMAGES.iter().find(|(_, mots)| mots.iter().any(|m| plie.contains(m))) {
        return Some(image);
    }
    match destination {
        Some(Destination::Outlook) => Some("lettre"),
        Some(Destination::Web | Destination::Chrome) => Some("globe"),
        _ => None,
    }
}

/// Une couleur [r, v, b] de `sujet.js`.
fn rgb_de(v: &Value) -> Option<Rgb> {
    let a = v.as_array()?;
    let c = |k: usize| a.get(k).and_then(Value::as_f64).map(|x| x.round().clamp(0.0, 255.0) as u8);
    Some([c(0)?, c(1)?, c(2)?])
}

/// Les outils d'un jour dans le journal d'Eve (`eve.log`, ou `eve.1.log` puis `eve.log` mis bout à bout), pour rejouer
/// la journée : ses lignes « outil ▸ NOM : détail » de la date (« 2026-10-23 »), chacune un début suivi de sa fin (le
/// journal ne note que le début) ; un « outil rejeté avant la garde » fait de la fin du dernier outil de ce nom une
/// erreur. Le détail est celui de l'écran (le chemin, la commande, le motif, 140 caractères au plus) ; les outils de la
/// base d'Entrepreneur Général (« base · sql ») reprennent leur nom entier.
pub fn journee(eve_log: &str, date: &str) -> Vec<Evenement> {
    let mut evenements: Vec<Evenement> = Vec::new();
    let date = date.trim();
    if date.is_empty() {
        return evenements;
    }
    for ligne in eve_log.lines() {
        let Some(reste) = ligne.strip_prefix(date).and_then(|r| r.strip_prefix(' ')) else { continue };
        let Some((heure, message)) = reste.split_once(' ') else { continue };
        if heure.len() != 8 || !heure.bytes().all(|b| b.is_ascii_digit() || b == b':') {
            continue;
        }
        if let Some(resume) = message.strip_prefix("outil ▸ ") {
            let (nom, detail) = resume.split_once(" : ").unwrap_or((resume, ""));
            let nom = nom.trim();
            if nom.is_empty() {
                continue;
            }
            let outil = match nom.strip_prefix("base · ") {
                Some(n) => format!("mcp__entrepreneur-general__{n}"),
                None => nom.to_string(),
            };
            let detail = detail.trim_end().strip_suffix(" […]").unwrap_or(detail).trim().to_string();
            let id = format!("journal-{}", evenements.len() / 2 + 1);
            evenements.push(Evenement {
                id: id.clone(),
                outil: outil.clone(),
                detail: detail.clone(),
                fin: false,
                erreur: false,
            });
            evenements.push(Evenement { id, outil, detail, fin: true, erreur: false });
        } else if let Some(reste) = message.strip_prefix("outil rejeté avant la garde (rien n'a tourné) : ") {
            let nom = reste.rsplit_once(" (").map_or(reste, |(n, _)| n).trim();
            if let Some(e) = evenements.iter_mut().rev().find(|e| e.fin && e.outil == nom) {
                e.erreur = true;
            }
        }
    }
    evenements
}

#[cfg(test)]
mod essais {
    use serde_json::json;

    use super::*;

    const FAMILLE: &str = r"C:\Exemple\Maison";

    fn ou(outil: &str, detail: &str) -> Destination {
        destination(outil, detail, Path::new(FAMILLE))
    }

    fn dossier(d: &str) -> Destination {
        Destination::Dossier(d.to_string())
    }

    fn evenement(id: &str, outil: &str, detail: &str, fin: bool, erreur: bool) -> Evenement {
        Evenement { id: id.into(), outil: outil.into(), detail: detail.into(), fin, erreur }
    }

    #[test]
    fn les_fichiers_du_dossier_de_travail_y_restent() {
        for (outil, detail) in [
            ("Read", r"C:\Exemple\Maison\agenda\ics\pedagogiques.ics"),
            ("Write", "c:/exemple/maison/famille.md"),
            ("Edit", r"\\?\C:\Exemple\Maison\CLAUDE.md"),
            ("Read", "/c/Exemple/Maison/activite/activite.js"),
            ("Glob", "**/*.md"),
            ("Glob", r"C:\Exemple\Maison\**\*.ics"),
            ("Grep", "journées pédagogiques"),
            ("Grep", r"\bNoah\b"),
            ("Grep", r"\d+\s*\$"),
            ("Read", "agenda/semaine.md"),
            ("Glob", r"C:\Exemple\Maison"),
        ] {
            assert_eq!(ou(outil, detail), Destination::Travail, "{outil} {detail}");
        }
        // Un nom qui commence comme le dossier de travail n'est pas dedans.
        assert_eq!(ou("Read", r"C:\Exemple\Maison2\x.md"), dossier(r"C:\Exemple\Maison2"));
    }

    #[test]
    fn un_autre_dossier_se_nomme_par_le_dossier_qui_le_dit() {
        // Le voisin du dossier de travail : la Comptabilité du modèle, quelle que soit l'écriture du chemin.
        assert_eq!(ou("Read", r"C:\Exemple\Comptabilité\plan-action.md"), dossier(r"C:\Exemple\Comptabilité"));
        assert_eq!(ou("Read", "/c/Exemple/Comptabilité/2025/releve.pdf"), dossier(r"C:\Exemple\Comptabilité"));
        assert_eq!(ou("Read", r"C:\Exemple\Maison\..\Comptabilité\x.md"), dossier(r"C:\Exemple\Comptabilité"));
        assert_eq!(ou("Glob", "../Comptabilité/**/*.csv"), dossier(r"C:\Exemple\Comptabilité"));
        assert_eq!(ou("Glob", "C:/Exemple/Comptabilité/**/*.pdf"), dossier(r"C:\Exemple\Comptabilité"));
        assert_eq!(ou("Grep", r"C:\Exemple\Comptabilité\factures.csv"), dossier(r"C:\Exemple\Comptabilité"));
        // Un fichier posé dans le dossier parent : le parent.
        assert_eq!(ou("Read", r"C:\Exemple\notes.md"), dossier(r"C:\Exemple"));
        // Ailleurs sur le disque : le premier dossier sous la racine ; sous Program Files, le programme.
        assert_eq!(ou("Read", r"C:\Dev\Eve\src\main.rs"), dossier(r"C:\Dev"));
        assert_eq!(ou("Read", r"D:\Photos\2025\noel.jpg"), dossier(r"D:\Photos"));
        assert_eq!(
            ou("Read", r"C:\Program Files\Microsoft Office\root\x.dll"),
            dossier(r"C:\Program Files\Microsoft Office")
        );
        assert_eq!(ou("Read", r"C:\notes.txt"), dossier(r"C:\"));
        assert_eq!(ou("Read", r"\\nas\partage\Factures\2025.pdf"), dossier(r"\\nas\partage\Factures"));
        // Le profil : son premier dossier ; sous AppData, celui du programme.
        assert_eq!(ou("Read", r"C:\Users\Sam\Documents\Impôts\2025.pdf"), dossier(r"C:\Users\Sam\Documents"));
        assert_eq!(
            ou("Read", r"C:\Users\Sam\AppData\Local\Eve\specialistes\assistante\eve.log"),
            dossier(r"C:\Users\Sam\AppData\Local\Eve")
        );
        assert_eq!(ou("Read", "~/Downloads/horaire.pdf"), dossier(r"~\Downloads"));
        // Pour Eve Designer (C:\Dev\blender-main), C:\Dev\Eve est un voisin.
        let designer = Path::new(r"C:\Dev\blender-main");
        assert_eq!(destination("Read", r"C:\Dev\Eve\src\main.rs", designer), dossier(r"C:\Dev\Eve"));
        assert_eq!(destination("Read", r"C:\Dev\blender-main\outils\eve\x.md", designer), Destination::Travail);
    }

    #[test]
    fn les_outils_sans_chemin_vont_a_leur_destination() {
        assert_eq!(ou("WebSearch", "calendrier scolaire 2026-2027"), Destination::Web);
        assert_eq!(ou("WebFetch", "https://www.ecoles.exemple.ca/calendrier"), Destination::Web);
        assert_eq!(ou("mcp__claude-in-chrome__navigate", "https://example.com"), Destination::Chrome);
        assert_eq!(
            ou("mcp__claude_ai_Constructo_AI__lister_factures", ""),
            Destination::Mcp("claude_ai_Constructo_AI".into())
        );
        assert_eq!(ou("mcp__freecad__freecad_exec", "Part.makeBox(1, 1, 1)"), Destination::Mcp("freecad".into()));
        assert_eq!(ou("PowerShell", "$o = New-Object -ComObject Outlook.Application"), Destination::Outlook);
        assert_eq!(ou("Bash", "python scripts/outlook_mail.py --json draft"), Destination::Outlook);
        assert_eq!(ou("Bash", "cd \"C:/Dev/Eve/src\"; sed -n 1,140p reglages.rs"), Destination::Commandes);
        assert_eq!(ou("PowerShell", "Get-Date"), Destination::Commandes);
        for outil in ["Skill", "ToolSearch", "Agent", "TodoWrite", "", "mcp____x"] {
            assert_eq!(ou(outil, "courriels"), Destination::Outils, "{outil}");
        }
    }

    #[test]
    fn les_noms_des_destinations() {
        let famille = Path::new(FAMILLE);
        assert_eq!(Destination::Travail.nom(famille), "DOSSIER MAISON");
        assert_eq!(Destination::Travail.nom(Path::new("")), "DOSSIER DE TRAVAIL");
        assert_eq!(dossier(r"C:\Exemple\Comptabilité").nom(famille), "COMPTABILITÉ");
        assert_eq!(dossier(r"C:\").nom(famille), "C:");
        assert_eq!(dossier(r"\\nas\partage").nom(famille), "PARTAGE");
        assert_eq!(Destination::Mcp("claude_ai_Constructo_AI".into()).nom(famille), "CONSTRUCTO AI");
        assert_eq!(Destination::Mcp("claude_ai_Claude_Docs".into()).nom(famille), "CLAUDE DOCS");
        assert_eq!(Destination::Mcp("entrepreneur-general".into()).nom(famille), "ENTREPRENEUR GENERAL");
        assert_eq!(Destination::Commandes.nom(famille), "POSTE · COMMANDES");
        // La casse ne fait pas un autre dossier.
        assert_eq!(dossier(r"C:\Exemple\Comptabilité").cle(), dossier(r"C:\EXEMPLE\comptabilité").cle());
        assert_ne!(
            dossier(r"C:\Exemple\Comptabilité").cle(),
            Destination::Mcp(r"C:\Exemple\Comptabilité".into()).cle()
        );
    }

    #[test]
    fn ce_que_porte_une_impulsion() {
        assert_eq!(etiquette("Read", r"C:\Exemple\Maison\agenda\ics\pedagogiques.ics"), "pedagogiques.ics");
        assert_eq!(etiquette("Glob", "C:/Exemple/Comptabilité/**/*.pdf"), "*.pdf");
        assert_eq!(etiquette("WebFetch", "https://www.ecoles.exemple.ca/calendrier?x=1"), "ecoles.exemple.ca");
        assert_eq!(etiquette("WebSearch", "calendrier   scolaire"), "calendrier scolaire");
        assert_eq!(etiquette("mcp__claude_ai_Constructo_AI__lister_factures", ""), "lister factures");
        assert_eq!(etiquette("Bash", "python scripts/journal.py seance \"Purge\""), "python scripts/journal.py…");
        assert_eq!(etiquette("Bash", "python scripts/journal.py seance \"Purge\"").chars().count(), 26);
        assert_eq!(verbe("Read"), "LECTURE");
        assert_eq!(verbe("mcp__claude-in-chrome__navigate"), "NAVIGATE");
    }

    fn camera(lacet: f32, tangage: f32) -> Camera {
        Camera { lacet, tangage, centre: pos2(400.0, 300.0), echelle: 200.0 }
    }

    fn proche(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn la_projection_en_perspective() {
        let c = camera(0.0, 0.0);
        // Le centre du réseau tombe au centre, à la profondeur zéro, sans grossir.
        let o = projeter([0.0, 0.0, 0.0], &c);
        assert_eq!((o.pos, o.z, o.s), (pos2(400.0, 300.0), 0.0, 1.0));
        // Sur le plan du centre, une unité vaut l'échelle ; l'axe y descend, comme celui de l'écran.
        let x = projeter([1.0, 0.0, 0.0], &c);
        assert!(proche(x.pos.x, 600.0) && proche(x.pos.y, 300.0) && proche(x.s, 1.0));
        assert!(proche(projeter([0.0, 1.0, 0.0], &c).pos.y, 500.0));
        // Devant (vers l'œil, z négatif), un point est plus grand et plus loin du centre ; derrière, plus petit.
        let (devant, derriere) = (projeter([0.5, 0.0, -1.0], &c), projeter([0.5, 0.0, 1.0], &c));
        assert!(proche(devant.s, 1.5) && proche(derriere.s, 0.75));
        assert!(proche(devant.pos.x, 400.0 + 0.5 * 200.0 * 1.5) && proche(derriere.pos.x, 400.0 + 0.5 * 200.0 * 0.75));
        // Un quart de tour de lacet envoie l'axe x derrière le centre.
        let q = projeter([1.0, 0.0, 0.0], &camera(std::f32::consts::FRAC_PI_2, 0.0));
        assert!(proche(q.pos.x, 400.0) && proche(q.z, 1.0) && proche(q.s, 0.75));
        // L'inclinaison du modèle (−0,35) : le devant du réseau monte à l'écran, l'arrière descend.
        let c = camera(0.0, -0.35);
        assert!(projeter([0.0, 0.0, -1.0], &c).pos.y < 300.0 && projeter([0.0, 0.0, 1.0], &c).pos.y > 300.0);
    }

    #[test]
    fn la_rotation_garde_les_longueurs() {
        let mut h = Hasard(7);
        for _ in 0..200 {
            let p = [h.entre(-1.3, 1.3), h.entre(-1.3, 1.3), h.entre(-1.3, 1.3)];
            let q = tourner(p, h.entre(-10.0, 10.0), h.entre(-1.35, 1.35));
            assert!((d2(p, [0.0; 3]) - d2(q, [0.0; 3])).abs() < 1e-4, "{p:?} → {q:?}");
        }
    }

    #[test]
    fn le_cerveau_est_celui_du_modele_et_d_un_seul_tenant() {
        let c = construire();
        // Les neurones de la page de l'Assistante famille (son tirage, sa graine), calculés par son propre code.
        for (i, attendu) in [
            (1, [-0.843414, 0.026214, 0.222465]),
            (2, [0.006529, 0.648248, -0.489995]),
            (330, [0.622133, 0.092557, -0.766201]),
        ] {
            assert!((0..3).all(|k| (c.places[i][k] - attendu[k]).abs() < 1e-5), "neurone {i} : {:?}", c.places[i]);
        }
        assert_eq!(c.places.len(), 1 + NEURONES + MAX_DESTINATIONS);
        // Eve atteint tous les neurones par les neurones seuls ; chaque place de destination en touche huit.
        assert!(atteints(&c.voisins)[..=NEURONES].iter().all(|&v| v));
        for k in 0..MAX_DESTINATIONS {
            let place = 1 + NEURONES + k;
            assert_eq!(c.voisins[place].len(), 8);
            assert!(c.voisins[place].iter().all(|&j| (1..=NEURONES).contains(&j)));
        }
        // L'ordre des places les donne toutes, une fois chacune ; les deux premières sont presque opposées.
        let mut ordre = c.ordre.clone();
        ordre.sort_unstable();
        assert_eq!(ordre, (1 + NEURONES..1 + NEURONES + MAX_DESTINATIONS).collect::<Vec<_>>());
        assert!(d2(c.places[c.ordre[0]], c.places[c.ordre[1]]).sqrt() > 2.0 * RAYON_DESTINATIONS * 0.95);
        // Le même tirage à chaque fois.
        let d = construire();
        assert_eq!((c.places, c.synapses), (d.places, d.synapses));
    }

    #[test]
    fn une_impulsion_suit_les_synapses() {
        let mut r = Reseau::nouveau(Path::new(FAMILLE));
        for &cible in &r.ordre.clone() {
            let chemin = r.chemin(cible);
            assert_eq!((chemin[0], *chemin.last().unwrap()), (0, cible));
            assert!(chemin.windows(2).all(|w| r.voisins[w[0]].contains(&w[1])));
            assert!(chemin[1..chemin.len() - 1].iter().all(|&j| (1..=NEURONES).contains(&j)));
        }
    }

    #[test]
    fn un_debut_puis_sa_fin() {
        let mut r = Reseau::nouveau(Path::new(FAMILLE));
        assert_eq!(r.poles.len(), 1, "le dossier de travail est là d'emblée");
        r.recevoir(&evenement("t1", "Read", r"C:\Exemple\Comptabilité\plan.md", false, false), 1.0);
        assert_eq!((r.total, r.poles.len(), r.poles[1].en_cours, r.en_cours.len()), (1, 2, 1, 1));
        assert_eq!(r.poles[1].nom, "COMPTABILITÉ");
        assert_eq!(r.poles[1].couleur, DOSSIERS[0]);
        assert_eq!(r.impulsions.last().map(|i| (i.aller, i.couleur)), Some((true, CYAN)));
        r.recevoir(&evenement("t1", "Read", r"C:\Exemple\Comptabilité\plan.md", true, false), 1.4);
        assert_eq!((r.total, r.poles[1].total, r.poles[1].en_cours, r.en_cours.len()), (1, 1, 0, 0));
        assert_eq!(r.impulsions.last().map(|i| (i.aller, i.couleur)), Some((false, VERT)));
        // Une fin en erreur revient en rouge et se compte ; une fin sans début est une action de plus.
        r.recevoir(&evenement("t2", "Edit", r"C:\Exemple\Maison\famille.md", false, false), 2.0);
        r.recevoir(&evenement("t2", "Edit", r"C:\Exemple\Maison\famille.md", true, true), 2.1);
        assert_eq!((r.poles[0].total, r.poles[0].erreurs), (1, 1));
        assert_eq!(r.impulsions.last().map(|i| (i.couleur, i.etiquette.as_str())), Some((ROUGE, "ERREUR")));
        r.recevoir(&evenement("t9", "WebSearch", "météo Montréal", true, false), 3.0);
        assert_eq!((r.total, r.poles.len()), (3, 3));
        // Un début resté sans fin depuis un quart d'heure ne compte plus parmi les outils en cours.
        r.recevoir(&evenement("t3", "Bash", "sleep 9999", false, false), 4.0);
        assert_eq!(r.en_cours.len(), 1);
        r.oublier(4.0 + OUBLI + 1.0);
        assert_eq!((r.en_cours.len(), r.poles[3].en_cours), (0, 0));
    }

    #[test]
    fn soixante_destinations_au_plus() {
        let mut r = Reseau::nouveau(Path::new(FAMILLE));
        for k in 0..70 {
            // Le même dossier deux fois, écrit autrement : une seule destination.
            r.recevoir(&evenement(&format!("a{k}"), "Read", &format!(r"D:\Dossier{k}\x.md"), false, false), 1.0);
            r.recevoir(&evenement(&format!("b{k}"), "Read", &format!("d:/DOSSIER{k}/y.md"), false, false), 1.0);
        }
        assert_eq!(r.poles.len(), MAX_DESTINATIONS);
        assert_eq!(r.poles[MAX_DESTINATIONS - 1].destination, Destination::Autres);
        // Le dossier de travail, 58 dossiers de deux actions chacun, puis « Autres » pour les 12 suivants.
        assert!(r.poles[1..MAX_DESTINATIONS - 1].iter().all(|p| p.total == 2));
        assert_eq!(r.poles[MAX_DESTINATIONS - 1].total, 2 * (70 - 58));
        assert_eq!(r.total, 140);
        let places: std::collections::HashSet<usize> = r.poles.iter().map(|p| p.place).collect();
        assert_eq!(places.len(), MAX_DESTINATIONS, "une place chacune");
    }

    #[test]
    fn le_reseau_se_dessine_sans_fenetre() {
        let ctx = egui::Context::default();
        let mut r = Reseau::nouveau(Path::new(FAMILLE));
        let image = |r: &mut Reseau, t: f64| {
            let entree = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(900.0, 640.0))),
                time: Some(t),
                ..Default::default()
            };
            let mut anime = false;
            let mut sortie = ctx.run_ui(entree, |ui| {
                let rect = ui.max_rect();
                anime = r.dessiner(ui, rect, t);
            });
            sortie.textures_delta.clear();
            let primitives = ctx.tessellate(std::mem::take(&mut sortie.shapes), sortie.pixels_per_point);
            let mut sommets = 0;
            for p in &primitives {
                if let egui::epaint::Primitive::Mesh(m) = &p.primitive {
                    assert!(m.vertices.iter().all(|v| v.pos.x.is_finite() && v.pos.y.is_finite()));
                    sommets += m.vertices.len();
                }
            }
            (anime, sommets)
        };
        // Au repos, le réseau se dessine (des milliers de sommets) sans rien demander de plus que les quatre images
        // par seconde.
        let (anime, sommets) = image(&mut r, 0.0);
        assert!(!anime && sommets > 5_000, "{anime} {sommets}");
        assert!(!image(&mut r, 0.25).0);
        // Des outils : le réseau s'anime, jusqu'à ce que tout soit revenu et éteint.
        for k in 0..20 {
            let (outil, detail) = if k % 2 == 0 {
                ("Read", format!(r"D:\Projet{k}\a.md"))
            } else {
                ("WebFetch", "https://exemple.ca/".into())
            };
            r.recevoir(&evenement(&format!("t{k}"), outil, &detail, false, false), 0.3);
        }
        for t in [0.3, 0.5, 1.0, 1.5, 2.0, 3.0] {
            assert!(image(&mut r, t).0, "à {t} s");
        }
        for k in 0..20 {
            r.recevoir(&evenement(&format!("t{k}"), "Read", "", true, k == 3), 3.2);
        }
        for t in [3.3, 4.0, 5.0, 6.0] {
            assert!(image(&mut r, t).0, "à {t} s");
        }
        assert!(!image(&mut r, 30.0).0);
        assert!(!image(&mut r, 30.25).0);
    }

    // ---- Le sujet, les personnes, la teinte, la ligne d'état, la courbe, les modes ----

    /// Des personnes d'exemple : deux parents, deux enfants ; le premier et le dernier ont un mot de plus.
    fn famille_d_exemple() -> Vec<Personne> {
        let p = |nom: &str, couleur: [u8; 3], motifs: &[&str]| Personne {
            nom: nom.into(),
            couleur,
            motifs: motifs.iter().map(|m| m.to_string()).collect(),
        };
        vec![
            p("Marc", [0, 229, 255], &["chantier"]),
            p("Julie", [255, 122, 184], &[]),
            p("Léa", [255, 210, 63], &[]),
            p("Noah", [95, 255, 196], &["garderie"]),
        ]
    }

    /// Une image sans fenêtre : le réseau s'anime-t-il, et ses sommets (tous finis).
    fn une_image(ctx: &egui::Context, r: &mut Reseau, t: f64) -> (bool, usize) {
        let entree = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(900.0, 640.0))),
            time: Some(t),
            ..Default::default()
        };
        let mut anime = false;
        let mut sortie = ctx.run_ui(entree, |ui| {
            let rect = ui.max_rect();
            anime = r.dessiner(ui, rect, t);
        });
        sortie.textures_delta.clear();
        let mut sommets = 0;
        for p in ctx.tessellate(std::mem::take(&mut sortie.shapes), sortie.pixels_per_point) {
            if let egui::epaint::Primitive::Mesh(m) = &p.primitive {
                let fini = m.vertices.iter().all(|v| v.pos.x.is_finite() && v.pos.y.is_finite());
                assert!(fini, "un sommet infini à {t} s (dessin {}, inclinaison {})", r.morph, r.tangage);
                sommets += m.vertices.len();
            }
        }
        (anime, sommets)
    }

    #[test]
    fn les_images_sont_celles_du_modele() {
        // Calculées par le code même de la page (ses lignes 99-209, sous node) : le nombre de traits et de points, la
        // longueur des traits, et la place des neurones 1, 2, 165 et 330.
        const MODELE: [(&str, usize, usize, f64); 10] = [
            ("ecole", 7, 43, 11.728388),
            ("piece", 4, 113, 13.709977),
            ("lettre", 4, 12, 9.717081),
            ("agenda", 8, 19, 14.1),
            ("croix", 1, 13, 6.8),
            ("globe", 6, 106, 15.790024),
            ("repas", 4, 102, 11.590201),
            ("maison", 10, 42, 11.509984),
            ("avion", 15, 101, 10.079928),
            ("soleil", 20, 108, 8.38731),
        ];
        const PLACES: [[[f32; 2]; 4]; 10] = [
            [[-0.8, 0.068577], [0.046788, 0.25], [-0.030881, 0.65], [0.72778, -0.208646]],
            [[-0.862481, -0.165091], [0.0, 0.239719], [-0.032303, 0.491924], [0.711899, 0.103737]],
            [[-0.92, -0.308852], [0.061447, 0.056586], [-0.188784, 0.55], [0.92, -0.37297]],
            [[-0.8, -0.317727], [0.056818, -0.3], [-0.173182, 0.85], [0.8, -0.484091]],
            [[-0.85, -0.079697], [0.069394, -0.85], [-0.213636, 0.85], [0.77303, 0.25]],
            [[-0.855378, 0.2008], [0.0, 0.281893], [-0.005031, -0.87967], [0.679826, 0.0]],
            [[-1.0, -0.41121], [0.053783, -0.846475], [-0.178924, -0.518844], [1.0, -0.406337]],
            [[-0.65, 0.047112], [0.12, 0.429792], [-0.068503, -0.598623], [0.629072, -0.178196]],
            [[-0.869264, -0.504741], [-0.053861, -0.19115], [-0.141807, -0.335309], [0.660973, 0.15]],
            [[-0.751716, -0.000006], [0.000008, -0.753776], [-0.052535, 0.294773], [0.629862, 0.260898]],
        ];
        let c = construire();
        for ((nom, n_traits, n_points, longueur), places) in MODELE.into_iter().zip(PLACES) {
            let t = traits(nom);
            assert_eq!((t.len(), t.iter().map(Vec::len).sum::<usize>()), (n_traits, n_points), "{nom}");
            let l: f64 =
                t.iter().flat_map(|t| t.windows(2)).map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1])).sum();
            assert!((l - longueur).abs() < 1e-5, "{nom} : {l}");
            let ci = cibles(nom, &c.places);
            for (i, [x, y]) in [1, 2, 165, 330].into_iter().zip(places) {
                assert!((ci[i][0] - x).abs() < 2e-5 && (ci[i][1] - y).abs() < 2e-5, "{nom}, neurone {i} : {:?}", ci[i]);
            }
        }
        // Toutes les images : chaque neurone a sa place, dans le carré de l'image ; « avions » prend les points
        // d'« avion ».
        for (nom, mot) in IMAGES {
            let ci = cibles(nom, &c.places);
            assert!(ci[1..].iter().all(|p| p[0].abs() <= 1.0 && p[1].abs() <= 1.0), "{nom}");
            assert!(!mot.is_empty() && nom_d_image(nom) == mot);
        }
        assert_eq!(cibles("avions", &c.places), cibles("avion", &c.places));
        assert!(image_connue("ballon").is_none() && traits("ballon").is_empty());
        // Les avions : les neurones pairs en haut, qui vont à droite ; les impairs en bas, retournés.
        let (a, b) = (avions([0.2, 0.1], 2, 0.0), avions([0.2, 0.1], 3, 0.0));
        assert!((a[0] - (0.1 - 1.4)).abs() < 1e-5 && (a[1] - (0.05 - 0.38)).abs() < 1e-5, "{a:?}");
        assert!(b[1] > 0.0 && avions([0.2, 0.1], 2, 1.0)[0] > a[0]);
    }

    #[test]
    fn ce_qui_fait_penser_a_une_image() {
        let sans = |texte: &str| sujet_de(&plier(texte), &[], None);
        // Dans l'ordre du modèle : l'école avant l'agenda, la santé avant l'agenda, le repas avant la semaine.
        assert_eq!(sans(r"C:\Exemple\Maison\agenda\ics\pedagogiques.ics"), Some("ecole"));
        assert_eq!(sans("Journées PÉDAGOGIQUES"), Some("ecole"));
        assert_eq!(sans(r"C:\Exemple\Comptabilité\plan-action.md"), Some("piece"));
        assert_eq!(sans("factures Hydro-Québec"), Some("piece"));
        assert_eq!(sans("rendez-vous chez le dentiste"), Some("croix"));
        assert_eq!(sans("menu de la semaine"), Some("repas"));
        assert_eq!(sans("calendrier scolaire 2026-2027"), Some("agenda"));
        assert_eq!(sans("python scripts/outlook_mail.py --json draft"), Some("lettre"));
        assert_eq!(sans(r"C:\Exemple\Maison\famille.md"), None);
        // À défaut, la destination : les courriels d'Outlook, le Web, Chrome.
        assert_eq!(sujet_de("get-date", &[], Some(&Destination::Outlook)), Some("lettre"));
        assert_eq!(sujet_de("https://exemple.ca", &[], Some(&Destination::Web)), Some("globe"));
        assert_eq!(sujet_de("navigate", &[], Some(&Destination::Chrome)), Some("globe"));
        assert_eq!(sujet_de("famille.md", &[], Some(&Destination::Travail)), None);
        // Les mots de la fenêtre passent avant ceux du modèle, en mots entiers.
        let mut r = Reseau::nouveau(Path::new(FAMILLE));
        r.evoquer("ecole", vec!["Léa".into(), " ".into()]);
        r.evoquer("ballon", vec!["Noah".into()]);
        assert_eq!(r.evocations, vec![("ecole", vec!["lea".to_string()])]);
        assert_eq!(r.imaginer(r"agenda\ics\2026-10-24 Lea vaccin.ics", None), Some("ecole"));
        assert_eq!(r.imaginer("Cléa", None), None);
        r.evoquer("ecole", Vec::new());
        assert!(r.evocations.is_empty());
        // Le nom d'un outil MCP dit aussi ce qu'il fait.
        r.recevoir(&evenement("m", "mcp__claude_ai_Constructo_AI__lister_factures", "", false, false), 1.0);
        assert_eq!(r.sujet_en_cours(), Some("piece"));
    }

    #[test]
    fn les_personnes_et_leurs_quartiers() {
        // À quatre, les quartiers du modèle : le signe de x, puis celui de z.
        let quatre = [secteur(-0.5, -0.5, 4), secteur(0.5, -0.5, 4), secteur(-0.5, 0.5, 4), secteur(0.5, 0.5, 4)];
        assert_eq!(quatre, [0, 1, 2, 3]);
        // Autrement, des parts égales autour de l'axe : chacune a ses neurones.
        let c = construire();
        for n in [1, 2, 3, 5, 8] {
            let mut compte = vec![0; n];
            for p in &c.places[1..=NEURONES] {
                compte[secteur(p[0], p[2], n)] += 1;
            }
            assert!(compte.iter().all(|&k| k > NEURONES / n / 2), "{n} : {compte:?}");
        }
        let mut r = Reseau::nouveau(Path::new(FAMILLE));
        let mut neuf = famille_d_exemple();
        neuf.extend(famille_d_exemple());
        neuf.push(Personne::default());
        r.personnes(neuf);
        assert_eq!(r.personnes.len(), MAX_PERSONNES);
        r.personnes(famille_d_exemple());
        assert!(r.quartiers[1..=NEURONES].iter().all(|&q| q < 4));
        // Un nom en mots entiers, avec ou sans accents ; un mot qui désigne quelqu'un.
        assert_eq!(r.nommer(r"C:\Exemple\Maison\agenda\Lea\horaire.md", 1.0), vec![2]);
        assert_eq!(r.nommer("la GARDERIE de Marc", 1.0), vec![0, 3]);
        assert!(r.nommer("Cléa, Marcel, Léandre", 1.0).is_empty());
        // Le quartier s'allume (15 % de l'écart toutes les 66 ms), reste 12 s, puis s'éteint.
        let e = r.eclairages[2];
        assert!(e.force(1.0) < 1e-6);
        assert!((e.force(1.4) - (1.0 - (-0.4 * ALLUMAGE).exp()) as f32).abs() < 1e-4);
        assert!(e.force(12.9) > 0.99 && e.force(14.5) < 0.05);
        assert_eq!(r.eclairages[1].force(5.0), 0.0);
        // Renommée avant de s'éteindre, elle repart de sa force du moment.
        r.nommer("Léa", 13.5);
        assert!(r.eclairages[2].niveau > 0.2 && r.eclairages[2].jusqu_a == 25.5);
        // Sa couleur se mêle à celle des neurones de son quartier.
        let i = (1..=NEURONES).find(|&i| r.quartiers[i] == 2).unwrap_or(1);
        let mut forces = [0.0; MAX_PERSONNES];
        forces[2] = 1.0;
        assert_eq!(r.couleur_neurone(i, 30.0, None, &forces), ([255, 210, 63], 1.0));
    }

    #[test]
    fn la_phrase_de_repos_dit_ce_que_fait_eve() {
        let mut r = Reseau::nouveau(Path::new(FAMILLE));
        // Au repos, une phrase nouvelle se tape tout de suite.
        r.repos("EVE S'EN OCCUPE", 5.0);
        r.jouer(5.0);
        let tapee = 5.0 + "EVE S'EN OCCUPE".chars().count() as f64 * FRAPPE;
        assert_eq!(r.ligne_d_etat(tapee + 0.1), format!("EVE S'EN OCCUPE{CURSEUR}"));
        // Un outil la remplace ; elle revient 1,5 s après lui.
        r.recevoir(&evenement("t1", "Read", r"C:\Exemple\Maison\notes.md", false, false), 10.0);
        r.recevoir(&evenement("t1", "Read", r"C:\Exemple\Maison\notes.md", true, false), 10.5);
        assert!(r.ligne_d_etat(11.9).starts_with("EVE CONSULTE"));
        r.jouer(12.0);
        assert_eq!(r.ligne, "EVE S'EN OCCUPE");
        // Eve change d'état sous une ligne d'écoute toute fraîche : elle reste lisible deux secondes.
        r.entendu("c'est quoi le menu de la semaine", 20.0);
        let ecoute = r.ligne.clone();
        assert_ne!(ecoute, "EVE S'EN OCCUPE");
        r.repos("EVE TE RÉPOND", 20.1);
        r.jouer(20.5);
        assert_eq!(r.ligne, ecoute);
        let lisible = 20.0 + ecoute.chars().count() as f64 * FRAPPE + LECTURE;
        r.jouer(lisible + 0.01);
        assert_eq!(r.ligne, "EVE TE RÉPOND");
    }

    #[test]
    fn la_ligne_d_etat_se_tape() {
        let mut r = Reseau::nouveau(Path::new(FAMILLE));
        r.personnes(famille_d_exemple());
        // La page s'ouvre sur « À L'ÉCOUTE », déjà tapé ; le curseur clignote (0,4 s allumé, 0,4 s éteint).
        assert_eq!(r.ligne_d_etat(0.0), format!("EVE À L'ÉCOUTE{CURSEUR}"));
        assert_eq!(r.ligne_d_etat(0.5), "EVE À L'ÉCOUTE ");
        // Un outil : ce qu'Eve fait, le fichier, pour qui, à quoi elle pense ; une lettre toutes les 22 ms.
        let horaire = r"C:\Exemple\Maison\agenda\Léa\horaire.md";
        r.recevoir(&evenement("t1", "Read", horaire, false, false), 10.0);
        let ligne = "EVE CONSULTE LE DOSSIER MAISON · horaire.md · POUR LÉA · PENSE : AGENDA";
        assert_eq!(r.ligne_d_etat(10.0), CURSEUR.to_string());
        assert_eq!(r.ligne_d_etat(10.0 + 5.5 * FRAPPE), format!("EVE C{CURSEUR}"));
        let fin = 10.0 + ligne.chars().count() as f64 * FRAPPE;
        assert_eq!(r.ligne_d_etat(fin - 0.5 * FRAPPE), format!("{}{CURSEUR}", &ligne[..ligne.len() - 1]));
        assert_eq!(r.ligne_d_etat(fin + 0.1), format!("{ligne}{CURSEUR}"));
        assert_eq!(r.ligne_d_etat(fin + 0.5), format!("{ligne} "));
        // Sa fin : « À L'ÉCOUTE » 1,5 s plus tard, s'il ne vient pas d'autre outil d'ici là.
        r.recevoir(&evenement("t1", "Read", horaire, true, false), 12.0);
        assert!(r.ligne_d_etat(13.4).starts_with(ligne));
        assert_eq!(r.ligne_d_etat(13.5 + 3.5 * FRAPPE), format!("EVE{CURSEUR}"));
        r.jouer(14.0);
        assert_eq!((r.ligne.as_str(), r.ligne_depuis, r.a_l_ecoute), (A_L_ECOUTE, 13.5, None));
        r.recevoir(&evenement("t2", "WebSearch", "météo", false, false), 20.0);
        r.recevoir(&evenement("t2", "WebSearch", "météo", true, false), 20.5);
        r.recevoir(&evenement("t3", "mcp__claude_ai_Constructo_AI__lister_factures", "", false, false), 21.0);
        assert_eq!(r.ligne_d_etat(23.0), format!("EVE INTERROGE CONSTRUCTO AI · PENSE : ARGENT{CURSEUR}"));
        // Les autres phrases du modèle, et un autre dossier, qu'on lit ou dans lequel on écrit.
        for (outil, detail, debut) in [
            ("PowerShell", "$o = New-Object -ComObject Outlook.Application", "EVE OUVRE LES COURRIELS OUTLOOK · $o"),
            ("Bash", "Get-Date", "EVE EXÉCUTE UNE COMMANDE SUR LE POSTE · Get-Date"),
            ("Skill", "", "EVE PRÉPARE UN OUTIL"),
            ("Read", r"C:\Exemple\Comptabilité\plan.md", "EVE LIT LE DOSSIER COMPTABILITÉ · plan.md"),
            ("Write", r"C:\Exemple\Comptabilité\plan.md", "EVE ÉCRIT DANS LE DOSSIER COMPTABILITÉ · plan.md"),
            ("WebFetch", "https://www.ecoles.exemple.ca/x", "EVE CHERCHE SUR LE WEB · ecoles.exemple.ca"),
        ] {
            r.recevoir(&evenement(outil, outil, detail, false, false), 30.0);
            assert!(r.ligne.starts_with(debut), "{}", r.ligne);
        }
        // Ce qu'on imagine juste après un outil : sa ligne reste, « À L'ÉCOUTE » ne la remplace pas.
        let mut r = Reseau::nouveau(Path::new(FAMILLE));
        r.recevoir(&evenement("t1", "Read", "notes.md", false, false), 40.0);
        r.recevoir(&evenement("t1", "Read", "notes.md", true, false), 40.5);
        r.entendu("le menu de la semaine", 41.0);
        assert!(r.ligne_d_etat(45.0).starts_with("EVE ÉCOUTE · IMAGINE : REPAS"), "{}", r.ligne_d_etat(45.0));
    }

    #[test]
    fn sujet_js_ne_joue_qu_une_fois() {
        let mut r = Reseau::nouveau(Path::new(FAMILLE));
        r.personnes(famille_d_exemple());
        let maintenant = 1_792_787_700.0;
        // Plus vieux que 45 s, ou sans instant : rien.
        r.sujet_js(&json!({ "s": "avions", "t": maintenant - 46.0 }), maintenant, 1.0);
        r.sujet_js(&json!({ "s": "avions" }), maintenant, 1.0);
        assert!(r.sujet.is_none() && r.sujet_js_vu.is_none());
        // Une image, sa couleur (20 s) et une personne (12 s) ; la ligne d'état le dit.
        let avions = json!({ "s": "avions", "c": [90, 190, 255], "p": 2, "t": maintenant - 5.0 });
        r.sujet_js(&avions, maintenant, 1.0);
        assert_eq!(r.sujet.as_ref().map(|s| (s.nom, s.jusqu_a)), Some(("avions", 16.0)));
        assert_eq!(r.couleur, Some(([90, 190, 255], 21.0)));
        assert_eq!(r.eclairages[2].jusqu_a, 13.0);
        assert!(r.ligne_d_etat(5.0).starts_with("EVE ÉCOUTE · IMAGINE : AVIONS"));
        // La même valeur relue ne rejoue pas.
        r.sujet("aucun", 0.0, 2.0);
        r.sujet_js(&avions, maintenant, 2.0);
        assert!(r.sujet.is_none());
        // Une couleur seule : 40 s, ou sa durée.
        r.sujet_js(&json!({ "c": [255, 200, 40], "t": maintenant - 1.0 }), maintenant, 3.0);
        assert_eq!(r.couleur, Some(([255, 200, 40], 43.0)));
        assert!(r.ligne_d_etat(9.0).starts_with("EVE ÉCOUTE · TOUT EN COULEUR"));
        r.sujet_js(&json!({ "c": [10, 20, 300], "d": 5000, "p": [0, 1, 9], "t": maintenant }), maintenant, 4.0);
        assert_eq!(r.couleur, Some(([10, 20, 255], 9.0)));
        assert!(r.eclairages[0].jusqu_a == 16.0 && r.eclairages[1].jusqu_a == 16.0);
        // Une image et sa durée ; « aucun » la défait ; une image inconnue ne fait rien (et pourra jouer plus tard).
        r.sujet_js(&json!({ "s": "soleil", "d": 30000, "t": maintenant + 1.0 }), maintenant, 5.0);
        assert_eq!(r.sujet.as_ref().map(|s| (s.nom, s.jusqu_a)), Some(("soleil", 35.0)));
        r.sujet_js(&json!({ "s": "aucun", "p": 3, "t": maintenant + 2.0 }), maintenant, 6.0);
        assert!(r.sujet.is_none() && r.eclairages[3].jusqu_a < 0.0);
        r.sujet_js(&json!({ "s": "ballon", "t": maintenant + 3.0 }), maintenant, 7.0);
        assert!(r.sujet.is_none() && r.sujet_js_vu == Some(maintenant + 2.0));
        // La teinte : 88 % de la couleur, qui se fond sur les trois dernières secondes.
        r.teinte([255, 0, 0], 10.0, 100.0);
        assert_eq!(r.teinte_a(105.0), Some(([255, 0, 0], 0.88)));
        assert!(r.teinte_a(108.5).is_some_and(|(_, k)| (k - 0.44).abs() < 1e-4));
        assert_eq!(r.teinte_a(110.0), None);
        // Ce qu'on dit : qui est nommé s'allume, le réseau imagine.
        r.entendu("le dentiste de Julie", 200.0);
        assert_eq!(r.sujet_en_cours(), Some("croix"));
        assert_eq!(r.eclairages[1].jusqu_a, 212.0);
        assert!(r.ligne_d_etat(205.0).starts_with("EVE ÉCOUTE · IMAGINE : SANTÉ"));
    }

    /// Le réseau pense à 30 images par seconde, de `t` jusqu'à `fin`.
    fn penser_jusqu_a(r: &mut Reseau, t: &mut f64, fin: f64) {
        while *t < fin - 1e-9 {
            *t += 1.0 / 30.0;
            r.penser(*t, 1.0 / 30.0, false);
        }
    }

    #[test]
    fn le_dessin_vient_puis_repart() {
        let mut r = Reseau::nouveau(Path::new(FAMILLE));
        let tangage = r.tangage;
        let tour = std::f64::consts::TAU;
        r.lacet = 0.6 + 3.0 * tour;
        assert!(r.sujet("ecole", 10.0, 0.0) && !r.sujet("ballon", 10.0, 0.0));
        let mut t = 0.0;
        // L'image vient en 3 s environ ; le réseau se met de face.
        penser_jusqu_a(&mut r, &mut t, 2.5);
        assert!(r.forme.as_ref().is_some_and(|f| f.nom == "ecole") && r.morph > 0.8 && r.morph < 1.0);
        penser_jusqu_a(&mut r, &mut t, 6.0);
        assert_eq!(r.morph, 1.0);
        assert!((r.lacet - 3.0 * tour).abs() < 0.02 && r.tangage.abs() < 0.02, "{} {}", r.lacet, r.tangage);
        // Elle se défait à son heure, revient à la sphère en 2,4 s environ, et le réseau retrouve son inclinaison.
        penser_jusqu_a(&mut r, &mut t, 9.9);
        assert!(r.sujet.is_some() && r.morph == 1.0);
        penser_jusqu_a(&mut r, &mut t, 12.0);
        assert!(r.sujet.is_none() && r.morph > 0.0);
        penser_jusqu_a(&mut r, &mut t, 12.6);
        assert_eq!(r.morph, 0.0);
        penser_jusqu_a(&mut r, &mut t, 20.0);
        assert!(r.tangage == tangage && r.tangage_repos.is_none());
        // Une autre image pendant qu'une se montre : elle attend que la première se soit défaite.
        r.sujet("piece", 30.0, t);
        penser_jusqu_a(&mut r, &mut t, 22.0);
        r.sujet("lettre", 30.0, t);
        penser_jusqu_a(&mut r, &mut t, 22.5);
        assert!(r.forme.as_ref().is_some_and(|f| f.nom == "piece") && r.morph < 0.9);
        penser_jusqu_a(&mut r, &mut t, 26.0);
        assert!(r.forme.as_ref().is_some_and(|f| f.nom == "lettre") && r.morph > 0.5);
        // Après une longue absence, le dessin est où il doit être (le temps entier compte).
        r.sujet("aucun", 0.0, t);
        r.penser(t + 60.0, 60.0, false);
        assert!(r.morph == 0.0 && r.forme.is_some());
        r.penser(t + 60.1, 0.1, false);
        assert!(r.forme.is_none());
        // La voix se lisse (30 % de l'écart toutes les 45 ms).
        r.voix(1.0);
        r.penser(t + 61.0, 0.045, false);
        assert!((r.voix - 0.3).abs() < 1e-5);
        r.voix(f32::NAN);
        r.penser(t + 70.0, 9.0, false);
        assert_eq!(r.voix, 0.0);
    }

    #[test]
    fn la_courbe_d_activite() {
        let mut r = Reseau::nouveau(Path::new(FAMILLE));
        assert_eq!(r.activite(0.0), vec![0.0; CASES]);
        // Un début et sa fin : deux signaux dans une case ; six dans la suivante.
        r.recevoir(&evenement("a", "Read", "x.md", false, false), 10.0);
        r.recevoir(&evenement("a", "Read", "x.md", true, false), 10.1);
        for k in 0..3 {
            r.recevoir(&evenement(&format!("b{k}"), "Read", "x.md", false, false), 10.6);
            r.recevoir(&evenement(&format!("b{k}"), "Read", "x.md", true, false), 10.6);
        }
        let a = r.activite(10.6);
        assert_eq!(a.len(), CASES);
        assert_eq!(&a[CASES - 2..], &[2.0 / 6.0, 1.0]);
        // Les cases glissent avec le temps, et s'effacent après 63 s.
        assert_eq!(&r.activite(11.3)[CASES - 3..], &[2.0 / 6.0, 1.0, 0.0]);
        assert_eq!(r.activite(10.6 + 63.0), vec![0.0; CASES]);
        // La plus haute vaut au moins trois signaux.
        let mut r = Reseau::nouveau(Path::new(FAMILLE));
        r.recevoir(&evenement("a", "Read", "x.md", false, false), 1.0);
        assert_eq!(r.activite(1.0)[CASES - 1], 1.0 / 3.0);
    }

    #[test]
    fn le_journal_du_jour() {
        let journal = "\
2026-10-22 23:59:58 outil ▸ Read : C:\\Exemple\\Maison\\hier.md
2026-10-23 08:00:01 Eve 0.1.0 (b441ca4) démarre (Assistante famille).
2026-10-23 08:01:02 outil ▸ Read : C:\\Exemple\\Maison\\famille.md
2026-10-23 08:01:05 outil ▸ Bash : python scripts/matin.py --poste […]
2026-10-23 08:01:09 outil ▸ base · sql : SELECT nom FROM projets
2026-10-23 08:01:10 outil rejeté avant la garde (rien n'a tourné) : mcp__entrepreneur-general__sql (toolu_01)
2026-10-23 08:02:00 outil ▸ WebSearch\r
2026-10-23 8:02:00 outil ▸ Read : mal formé
2026-10-24 00:00:01 outil ▸ Read : demain.md
";
        let j = journee(journal, "2026-10-23");
        let debuts: Vec<(&str, &str)> =
            j.iter().filter(|e| !e.fin).map(|e| (e.outil.as_str(), e.detail.as_str())).collect();
        assert_eq!(
            debuts,
            vec![
                ("Read", r"C:\Exemple\Maison\famille.md"),
                ("Bash", "python scripts/matin.py --poste"),
                ("mcp__entrepreneur-general__sql", "SELECT nom FROM projets"),
                ("WebSearch", ""),
            ]
        );
        // Chaque début est suivi de sa fin, du même identifiant ; l'outil rejeté finit en erreur.
        assert_eq!(j.len(), 8);
        assert!(j.chunks(2).all(|p| !p[0].fin && p[1].fin && p[0].id == p[1].id));
        let erreurs: Vec<&str> = j.iter().filter(|e| e.erreur).map(|e| e.outil.as_str()).collect();
        assert_eq!(erreurs, vec!["mcp__entrepreneur-general__sql"]);
        assert!(journee(journal, "").is_empty() && journee(journal, "2026-10-21").is_empty());
    }

    #[test]
    fn la_demonstration_et_la_journee_rejouee() {
        let mut r = Reseau::nouveau(Path::new(FAMILLE));
        r.demo(true);
        assert!(r.modes().demo);
        // La première image fixe l'heure ; puis un faux outil toutes les 0,8 s, qui finit de 1,3 à 2,2 s plus tard.
        r.jouer(10.0);
        assert_eq!(r.total, 0);
        for k in 1..=10 {
            r.jouer(10.01 + 0.4 * f64::from(k));
        }
        assert_eq!(r.total, 5);
        assert!((1..=3).contains(&r.en_cours.len()), "{}", r.en_cours.len());
        // Arrêtée, plus de faux outil ; ceux qui sont partis finissent.
        r.demo(false);
        for k in 1..=10 {
            r.jouer(14.01 + 0.4 * f64::from(k));
        }
        assert_eq!(r.total, 5);
        assert!(r.en_cours.is_empty() && r.simulees.is_empty());
        // Après une absence, la démonstration reprend sans rattraper le temps perdu.
        r.demo(true);
        r.jouer(20.0);
        r.jouer(100.0);
        assert_eq!(r.total, 5);
        r.demo(false);
        // La journée rejouée : les compteurs repartent de zéro, une ligne toutes les 0,26 s, les 150 dernières.
        let journal: String = (0..100)
            .map(|k| format!("2026-10-23 08:00:{:02} outil ▸ Read : C:\\Exemple\\Maison\\f{k}.md\n", k % 60))
            .collect();
        let j = journee(&journal, "2026-10-23");
        assert_eq!(j.len(), 200);
        r.rejouer(j.clone(), 200.0);
        assert!(r.modes().rediffusion && r.total == 0 && r.poles.iter().all(|p| p.total == 0));
        r.jouer(200.0);
        assert_eq!(r.total, 1);
        for k in 1..=10 {
            r.jouer(200.0 + 0.26 * 9.5 * f64::from(k) / 10.0);
        }
        assert_eq!(r.total, 5);
        for k in 10..200 {
            r.jouer(200.0 + 0.26 * f64::from(k));
        }
        assert_eq!(r.total, 75);
        assert!(!r.modes().rediffusion && r.en_cours.is_empty());
        // Une liste vide l'arrête.
        r.rejouer(j, 300.0);
        r.rejouer(Vec::new(), 300.0);
        assert!(!r.modes().rediffusion);
    }

    #[test]
    fn le_mode_econome() {
        let mut r = Reseau::nouveau(Path::new(FAMILLE));
        // Des images légères : rien ; plus lourdes que le seuil pendant 120 images : il se met de lui-même, et y reste.
        for _ in 0..500 {
            r.mesurer(1.0);
        }
        assert!(!r.modes().econome);
        for _ in 0..300 {
            r.mesurer(SEUIL_ECONOME * 3.0);
        }
        assert_eq!(r.modes(), Modes { econome: true, econome_de_lui_meme: true, ..Modes::default() });
        for _ in 0..500 {
            r.mesurer(1.0);
        }
        assert!(r.modes().econome);
        // À la main : il se retire, ou se met.
        r.econome(false);
        assert!(!r.modes().econome);
        r.econome(true);
        assert_eq!(r.modes(), Modes { econome: true, ..Modes::default() });
        // Une image d'un quart de seconde (la fenêtre était cachée) ne compte pas.
        let mut r = Reseau::nouveau(Path::new(FAMILLE));
        for _ in 0..300 {
            r.mesurer(400.0);
        }
        assert!(!r.modes().econome);
    }

    #[test]
    fn le_reseau_pense_sans_fenetre() {
        // Chaque image, une couleur, des personnes, la voix, le mode économe, la démonstration : des sommets finis, et
        // le réseau s'anime tant qu'il dessine ; puis il revient au repos.
        let ctx = egui::Context::default();
        let mut r = Reseau::nouveau(Path::new(FAMILLE));
        r.personnes(famille_d_exemple());
        r.teinte([255, 200, 40], 30.0, 0.0);
        r.voix(0.8);
        let mut t = 0.0;
        for (k, (nom, _)) in IMAGES.iter().enumerate() {
            r.sujet(nom, 3.5, t);
            if k == 5 {
                r.econome(true);
                r.demo(true);
            }
            // Un outil qui nomme une personne, sans autre image.
            r.recevoir(&evenement(&format!("p{k}"), "Read", r"C:\Exemple\Maison\Léa\notes.md", false, false), t);
            r.recevoir(&evenement(&format!("p{k}"), "Read", r"C:\Exemple\Maison\Léa\notes.md", true, false), t);
            for _ in 0..30 {
                t += 0.2;
                let (anime, sommets) = une_image(&ctx, &mut r, t);
                assert!(anime && sommets > 2_000, "{nom} à {t} s : {sommets}");
            }
        }
        r.demo(false);
        r.voix(0.0);
        let (mut anime, mut n) = (true, 0);
        while anime && n < 200 {
            t += 0.5;
            n += 1;
            anime = une_image(&ctx, &mut r, t).0;
        }
        assert!(!anime, "le réseau revient au repos");
    }
}
