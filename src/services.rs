//! Les services en ligne, pour le tableau de bord (le thème « Constructo AI ») : l'état de Claude (Anthropic), de
//! GitHub et de Render d'après leurs pages d'état publiques (Statuspage : `/api/v2/summary.json`, sans clé), et les
//! erreurs des journaux des services et des bases de données de Render (son API, `/v1/logs`, avec la clé d'API que
//! garde le dossier local : `cles.rs`). Seulement chez qui a posé une clé de Render (`eve --cle render`) : sans elle,
//! rien ne se lit, et le tableau n'en montre rien.
//!
//! Le suivi, comme la météo (`meteo.rs`) : un fil, une lecture tout de suite, puis toutes les 5 min ; après une
//! lecture manquée, une autre dans 1 min, puis 2, 4 et 8. Le fil regarde la clé toutes les 30 s (la date de
//! `render.cle`) : une clé posée ou changée se lit aussitôt, une clé retirée efface le relevé.
//!
//! Les pages d'état se lisent sans clé et sous un nom neutre (`https::get_public`), sans suivre de redirection :
//! `status.anthropic.com` renvoie à `status.claude.com`, que [`PAGES`] nomme. Une page manquée garde sa dernière
//! lecture une heure.
//!
//! Render porte la production de l'entreprise, et sa clé peut tout y faire, jusqu'à supprimer un service : Eve n'y fait
//! que des GET, sur les seules routes de [`ROUTES_LUES`], par une seule fonction, dont le verbe est écrit en dur
//! ([`obtenir_de_render`]). Ses services et ses bases (les suspendus sautés), puis, pour chacun, les lignes de son
//! journal au niveau « error » des dernières 24 h — les cent plus récentes au plus (`plus` au-delà). Une clé refusée se
//! dit, avec la commande qui la repose.
//!
//! Une réponse HTTP 5xx d'un service web est une ligne de requête (`type=request`, son code dans l'étiquette
//! `statusCode`), dont le niveau, Render le choisit : vu le 27 octobre 2026, « info » pour une 2xx ou une 3xx,
//! « warning » pour une 4xx ; celui d'une 5xx n'a pas pu se voir, et sa documentation ne le dit pas. Eve lit donc aussi
//! les lignes de requête d'un service web dont le code porte un 5 — le filtre de Render cherche un morceau du code
//! (« 4* » y rend des 304) : elle ne garde que les 500 à 599 —, et compte à part celles qui ne sont pas au niveau
//! « error » ([`Reponses5xx`]) : une 5xx au niveau « error » est déjà comptée parmi les erreurs.
//!
//! Au journal : des nombres et des états — jamais une ligne d'un journal de Render, ni le nom d'une ressource, ni la
//! clé.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::Value;

use crate::cles::{self, Service};
use crate::{centre, connexion, entreprise, https, journal, reglages};

/// Les pages d'état que lit Eve : le nom du service, l'adresse de sa page (à ouvrir d'un clic), celle de son résumé.
pub const PAGES: [(&str, &str, &str); 3] = [
    ("Claude", "https://status.claude.com", "https://status.claude.com/api/v2/summary.json"),
    ("GitHub", "https://www.githubstatus.com", "https://www.githubstatus.com/api/v2/summary.json"),
    ("Render", "https://status.render.com", "https://status.render.com/api/v2/summary.json"),
];

/// La fenêtre des erreurs de Render : les dernières 24 heures.
pub const FENETRE: Duration = Duration::from_secs(24 * 3600);

/// Les seules routes de l'API de Render qu'Eve lit, en GET : la liste des services, celle des bases Postgres, les
/// journaux. Une route s'ajoute ici, jamais ailleurs : [`adresse_de_render`] refuse toute autre avant l'envoi.
pub const ROUTES_LUES: [&str; 3] = ["/v1/services", "/v1/postgres", "/v1/logs"];

/// L'API de Render.
const API: &str = "https://api.render.com";
/// Une lecture toutes les 5 minutes.
const PERIODE: Duration = Duration::from_secs(5 * 60);
/// Après une lecture manquée, une autre dans 1 min, puis 2, 4, 8, et 8 ensuite : le réseau qui revient au réveil du
/// poste se voit vite ; une longue panne (le réseau coupé, Render qui demande d'attendre) envoie moins de requêtes
/// qu'une lecture ordinaire.
const REPRISE: Duration = Duration::from_secs(60);
const REPRISE_MAX: Duration = Duration::from_secs(8 * 60);
/// Le fil regarde la clé et l'horloge toutes les 30 s, plutôt que d'attendre 5 min d'un coup : une clé posée se lit
/// vite, et une attente de Windows ne compte pas le temps où le poste dort.
const PAS: Duration = Duration::from_secs(30);
/// Une page manquée garde sa dernière lecture une heure ; au-delà, sa panne se montre.
const VALIDITE: Duration = Duration::from_secs(60 * 60);
/// Le délai d'une page d'état (celui de la météo).
const DELAI: Duration = Duration::from_secs(20);
/// Le délai d'une requête à Render : son journal met parfois plus de 20 s à répondre (vu le 27 octobre 2026, deux
/// requêtes sur huit) ; une requête qui n'aboutit pas se refait une fois ([`obtenir_de_render`]).
const DELAI_RENDER: Duration = Duration::from_secs(45);
/// Au plus une lecture par minute, tous suivis confondus : une fenêtre qui relancerait le suivi à chaque image
/// n'écrirait pas pour autant trente fois par seconde à Render.
const ENTRE_DEUX_LECTURES: Duration = Duration::from_secs(60);
/// Les lignes d'une lecture d'un journal de Render : le plus qu'il en rende d'un coup.
const LIGNES_PAR_LECTURE: usize = 100;
/// La liste des services ou des bases : une page, la plus grande que rende Render.
const RESSOURCES_PAR_LISTE: usize = 100;
/// Ce que garde un état : les incidents en cours (une page en a rarement plus d'un ou deux), les composants touchés.
const INCIDENTS_MAX: usize = 10;
const COMPOSANTS_MAX: usize = 40;
/// Un nom (d'incident, de composant, de ressource), le message d'une ligne : pas plus sur le tableau.
const NOM_MAX: usize = 120;
const MESSAGE_MAX: usize = 300;
/// Une clé refusée, et ce qu'il faut faire.
const CLE_REFUSEE: &str = "la clé de Render est refusée (HTTP 401) — reposer la clé : eve --cle render";
const ACCES_REFUSE: &str =
    "Render refuse cette lecture à la clé (HTTP 403) — reposer celle d'un compte de l'équipe : eve --cle render";
/// Les débuts des clés et des jetons qu'une ligne de journal pourrait citer : le mot qui en porte un ne s'affiche pas
/// (le « Bearer » et les jetons JWT : `connexion::sans_porteur`).
const SECRETS: [&str; 10] =
    ["sk_live_", "sk_test_", "rk_live_", "rk_test_", "rnd_", "whsec_", "ghp_", "gho_", "github_pat_", "sk-ant-"];

/// Le niveau d'un service, du meilleur au pire : l'indicateur de sa page (Statuspage : `none`, `maintenance`, `minor`,
/// `major`, `critical`), l'impact d'un incident, ou l'état d'un composant (`operational`, `under_maintenance`,
/// `degraded_performance`, `partial_outage`, `major_outage`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Niveau {
    Operationnel,
    Maintenance,
    Mineur,
    Majeur,
    Critique,
}

impl Niveau {
    /// Le niveau que dit un mot de Statuspage — l'indicateur d'une page, l'impact d'un incident, l'état d'un
    /// composant : leurs mots ne se recoupent pas ; un mot inconnu n'en dit aucun.
    fn de(mot: &str) -> Option<Niveau> {
        Some(match mot.trim() {
            "none" | "operational" => Niveau::Operationnel,
            "maintenance" | "under_maintenance" => Niveau::Maintenance,
            "minor" | "degraded_performance" => Niveau::Mineur,
            "major" | "partial_outage" => Niveau::Majeur,
            "critical" | "major_outage" => Niveau::Critique,
            _ => return None,
        })
    }

    /// Ce qu'il est, en mots, d'après ceux de Statuspage (« Minor Service Outage », « Partial System Outage », « Major
    /// System Outage ») : pour le journal et l'essai.
    pub fn libelle(self) -> &'static str {
        match self {
            Niveau::Operationnel => "opérationnel",
            Niveau::Maintenance => "en maintenance",
            Niveau::Mineur => "perturbé",
            Niveau::Majeur => "en panne partielle",
            Niveau::Critique => "en panne majeure",
        }
    }
}

/// Un incident en cours (pas encore résolu) : son nom tel que la page l'écrit (en anglais, d'ordinaire), son impact, où
/// il en est (« investigating », « identified », « monitoring »), depuis quand (en secondes Unix), son lien court.
#[derive(Clone, Debug, PartialEq)]
pub struct Incident {
    pub nom: String,
    pub impact: Niveau,
    pub statut: String,
    pub depuis: i64,
    pub lien: String,
}

/// L'état d'un service d'après sa page : son niveau, ses incidents en cours (le plus grave d'abord), ses composants qui
/// ne vont pas bien (leur nom tel quel, leur niveau).
#[derive(Clone, Debug, PartialEq)]
pub struct Etat {
    pub niveau: Niveau,
    pub incidents: Vec<Incident>,
    pub composants: Vec<(String, Niveau)>,
}

/// Un service dont Eve lit la page d'état : son nom (« Claude »), l'adresse de sa page, son état à la dernière lecture
/// réussie (d'une heure au plus), sinon la raison de la panne.
#[derive(Clone, Debug, PartialEq)]
pub struct Statut {
    pub nom: String,
    pub page: String,
    pub etat: Result<Etat, String>,
}

/// La sorte d'une ressource de Render : un service web, une base de données Postgres, un autre service (un travailleur,
/// une tâche planifiée).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sorte {
    Web,
    Base,
    Autre,
}

/// Une ligne d'erreur d'un journal de Render : son heure (en secondes Unix), son message (une ligne, ébarbée, 300
/// caractères au plus).
#[derive(Clone, Debug, PartialEq)]
pub struct Ligne {
    pub quand: i64,
    pub message: String,
}

/// Les erreurs d'une ressource de Render dans la [`FENETRE`] — les lignes de ses journaux au niveau « error » : son nom
/// dans Render (ébarbé), sa sorte, combien (`plus` : davantage que la lecture n'en a compté), combien dans la dernière
/// heure, la plus récente ; et, à part, les réponses HTTP 5xx d'un service web que Render n'a pas mises au niveau
/// « error ».
#[derive(Clone, Debug, PartialEq)]
pub struct Erreurs {
    pub nom: String,
    pub sorte: Sorte,
    pub nombre: usize,
    pub plus: bool,
    pub derniere_heure: usize,
    pub derniere: Option<Ligne>,
    pub reponses_5xx: Reponses5xx,
}

/// Les réponses HTTP 5xx d'un service web dans la [`FENETRE`] (ses lignes de requête de code 500 à 599) qui ne sont
/// pas au niveau « error » — celles qui y sont comptent déjà parmi les erreurs : combien (`plus` : davantage que la
/// lecture n'en a vu), combien dans la dernière heure, la plus récente (« HTTP 502 · GET /api »). Rien pour une base ou
/// un autre service, qui ne répondent pas au Web.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Reponses5xx {
    pub nombre: usize,
    pub plus: bool,
    pub derniere_heure: usize,
    pub derniere: Option<Ligne>,
}

/// Un relevé des services : l'heure de sa lecture (en secondes Unix), l'état de chaque page ([`PAGES`], dans leur
/// ordre), et les erreurs de chaque ressource de Render (ses services web d'abord, puis ses bases ; les suspendus n'y
/// sont pas) — ou la raison d'une lecture de Render manquée (la clé refusée, l'API qui ne répond pas).
#[derive(Clone, Debug, PartialEq)]
pub struct Releve {
    pub lu_a: i64,
    pub statuts: Vec<Statut>,
    pub render: Result<Vec<Erreurs>, String>,
}

impl Releve {
    /// Ce que dit le relevé, en nombres et en états (le journal, l'essai) : jamais le nom d'une ressource ni une ligne
    /// de son journal.
    pub fn resume(&self) -> String {
        let pages: Vec<String> = self
            .statuts
            .iter()
            .map(|s| match &s.etat {
                Ok(e) => format!("{} {}", s.nom, e.niveau.libelle()),
                Err(_) => format!("{} sans page d'état", s.nom),
            })
            .collect();
        let render = match &self.render {
            Ok(liste) => resume_de_render(liste),
            Err(_) => "sur Render, la lecture manque".to_string(),
        };
        format!("{} ; {render}", pages.join(", "))
    }
}

/// Les ressources de Render et leurs erreurs, en nombres : « sur Render, 2 services web et 1 base : 7 erreurs en
/// 24 h, dont 0 dans la dernière heure » (et les réponses 5xx comptées à part, s'il y en a).
fn resume_de_render(liste: &[Erreurs]) -> String {
    let sortes: Vec<String> = [
        (Sorte::Web, "service web", "services web"),
        (Sorte::Base, "base", "bases"),
        (Sorte::Autre, "autre service", "autres services"),
    ]
    .into_iter()
    .map(|(s, un, plusieurs)| (liste.iter().filter(|e| e.sorte == s).count(), un, plusieurs))
    .filter(|(n, _, _)| *n > 0)
    .map(|(n, un, plusieurs)| nombre(n, un, plusieurs))
    .collect();
    if sortes.is_empty() {
        return "sur Render, aucune ressource".into();
    }
    let total: usize = liste.iter().map(|e| e.nombre).sum();
    let recentes: usize = liste.iter().map(|e| e.derniere_heure).sum();
    let au_moins = if liste.iter().any(|e| e.plus) { "au moins " } else { "" };
    let mut resume = format!(
        "sur Render, {} : {au_moins}{} en 24 h, dont {recentes} dans la dernière heure",
        et(&sortes),
        nombre(total, "erreur", "erreurs")
    );
    let reponses: usize = liste.iter().map(|e| e.reponses_5xx.nombre).sum();
    if reponses > 0 {
        let recentes: usize = liste.iter().map(|e| e.reponses_5xx.derniere_heure).sum();
        let au_moins = if liste.iter().any(|e| e.reponses_5xx.plus) { "au moins " } else { "" };
        let reponses = nombre(reponses, "réponse HTTP 5xx", "réponses HTTP 5xx");
        resume.push_str(&format!(" ; {au_moins}{reponses} hors des erreurs, dont {recentes} dans la dernière heure"));
    }
    resume
}

/// Des erreurs et des réponses 5xx, en mots : « 4 erreurs », « 1 réponse HTTP 5xx », « 4 erreurs et 2 réponses HTTP
/// 5xx ».
fn erreurs_en_mots(erreurs: usize, reponses: usize) -> String {
    let reponses_en_mots = || nombre(reponses, "réponse HTTP 5xx", "réponses HTTP 5xx");
    match (erreurs, reponses) {
        (_, 0) => nombre(erreurs, "erreur", "erreurs"),
        (0, _) => reponses_en_mots(),
        _ => format!("{} et {}", nombre(erreurs, "erreur", "erreurs"), reponses_en_mots()),
    }
}

/// « 0 erreur », « 1 erreur », « 3 erreurs ».
fn nombre(n: usize, un: &str, plusieurs: &str) -> String {
    format!("{n} {}", if n <= 1 { un } else { plusieurs })
}

/// « a », « a et b », « a, b et c ».
fn et(morceaux: &[String]) -> String {
    match morceaux.split_last() {
        None => String::new(),
        Some((dernier, [])) => dernier.clone(),
        Some((dernier, reste)) => format!("{} et {dernier}", reste.join(", ")),
    }
}

/// Un relevé imposé : la vitrine de `eve --essai-theme` (des services inventés). Tant qu'il l'est, tout suivi le rend
/// tel quel, sans fil ni requête. Eve ne l'impose jamais.
static IMPOSE: Mutex<Option<Releve>> = Mutex::new(None);

/// Impose ce relevé à tous les suivis, ceux qui tournent comme ceux qui viendront (la vitrine de l'essai du thème).
pub fn imposer(r: Releve) {
    *IMPOSE.lock().unwrap_or_else(PoisonError::into_inner) = Some(r);
}

fn impose() -> Option<Releve> {
    IMPOSE.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

/// Le suivi des services, que son fil tient à jour. Lâché, son fil s'arrête (à la fin d'une lecture en cours, s'il y
/// en a une).
pub struct Services {
    partage: Arc<Mutex<Option<Releve>>>,
    /// Lâché avec le suivi : son fil le voit et s'arrête.
    _arret: mpsc::Sender<()>,
}

impl Services {
    /// Lance le suivi, dans son fil (aucun sous un relevé imposé : [`imposer`]).
    pub fn lancer() -> Services {
        let partage: Arc<Mutex<Option<Releve>>> = Arc::default();
        let (arret, attente) = mpsc::channel();
        let p = Arc::clone(&partage);
        if impose().is_none()
            && let Err(e) = thread::Builder::new().name("services".into()).spawn(move || suivre(&p, &attente))
        {
            journal::ecrire(&format!("Services : le fil ne démarre pas : {e}"));
        }
        Services { partage, _arret: arret }
    }

    /// Le dernier relevé ; rien sans clé de Render, ni avant la première lecture. Un relevé imposé passe avant tout.
    pub fn releve(&self) -> Option<Releve> {
        impose().or_else(|| self.partage.lock().unwrap_or_else(PoisonError::into_inner).clone())
    }
}

// ---- Le fil -----------------------------------------------------------------------------------------------------

/// La dernière lecture réussie de chaque page ([`PAGES`], dans leur ordre) et son heure (en secondes Unix) : une page
/// manquée la garde une heure.
type Memoire = Vec<Option<(Etat, i64)>>;

/// Le fil du suivi : il regarde la clé toutes les 30 s, lit quand `Suivi` le dit, et s'arrête quand le suivi est lâché
/// (l'émetteur d'`arret` avec lui).
fn suivre(partage: &Mutex<Option<Releve>>, arret: &mpsc::Receiver<()>) {
    let chemin = cles::chemin(Service::Render);
    let mut suivi = Suivi::nouveau(SystemTime::now());
    let mut memoire: Memoire = vec![None; PAGES.len()];
    // La date de la clé au dernier regard ; `None` : pas encore regardée.
    let mut vue: Option<reglages::Date> = None;
    loop {
        let date = reglages::date_de(&chemin);
        if vue != Some(date) {
            if let Some(avant) = vue {
                journal::ecrire(match (avant.is_some(), date.is_some()) {
                    (false, true) => "Services : une clé de Render est posée : lecture.",
                    (true, false) => "Services : la clé de Render est retirée : le tableau n'en montre plus rien.",
                    _ => "Services : la clé de Render a changé : lecture.",
                });
            }
            vue = Some(date);
            suivi = Suivi::nouveau(SystemTime::now());
            if date.is_none() {
                *partage.lock().unwrap_or_else(PoisonError::into_inner) = None;
                memoire = vec![None; PAGES.len()];
            }
        }
        let maintenant = SystemTime::now();
        if date.is_some() && suivi.a_lire(maintenant) {
            match cles::lire(Service::Render) {
                None => {
                    if !suivi.cle_illisible_dite {
                        journal::ecrire(
                            "Services : la clé de Render ne se lit pas (posée sous une autre session de Windows ?) : \
                             rien ne se lit.",
                        );
                        suivi.cle_illisible_dite = true;
                    }
                    *partage.lock().unwrap_or_else(PoisonError::into_inner) = None;
                    suivi.prochaine = maintenant + PERIODE;
                }
                Some(cle) => match reserver() {
                    Err(reste) => suivi.prochaine = maintenant + reste,
                    Ok(()) => {
                        let l = lire_maintenant(&cle, &mut memoire);
                        for ligne in suivi.apres(&l, SystemTime::now()) {
                            journal::ecrire(&ligne);
                        }
                        *partage.lock().unwrap_or_else(PoisonError::into_inner) = Some(l.releve);
                    }
                },
            }
        }
        if !matches!(arret.recv_timeout(PAS), Err(mpsc::RecvTimeoutError::Timeout)) {
            return;
        }
    }
}

/// Le fil entre deux lectures : quand lire la suivante, et ce qui se dit au journal.
struct Suivi {
    prochaine: SystemTime,
    /// L'attente après une lecture manquée, qui double à chaque fois, jusqu'à [`REPRISE_MAX`].
    reprise: Duration,
    deja_lu: bool,
    /// Une panne ne se dit qu'une fois, jusqu'au retour : celle de Render, celle de chaque page.
    render_en_panne: bool,
    pages_en_panne: Vec<bool>,
    /// Le niveau de chaque page à la lecture d'avant : un changement se dit.
    niveaux: Vec<Option<Niveau>>,
    /// Des erreurs de Render dans la dernière heure, à la lecture d'avant.
    erreurs_recentes: bool,
    cle_illisible_dite: bool,
}

impl Suivi {
    fn nouveau(maintenant: SystemTime) -> Suivi {
        Suivi {
            prochaine: maintenant,
            reprise: REPRISE,
            deja_lu: false,
            render_en_panne: false,
            pages_en_panne: vec![false; PAGES.len()],
            niveaux: vec![None; PAGES.len()],
            erreurs_recentes: false,
            cle_illisible_dite: false,
        }
    }

    /// L'heure de lire est-elle venue ? Aussi quand l'horloge du poste a reculé de plus que la plus longue attente
    /// (une heure changée à la main) : la lecture suivante ne s'éloigne pas d'autant.
    fn a_lire(&self, maintenant: SystemTime) -> bool {
        let plus_longue = PERIODE.max(REPRISE_MAX);
        !matches!(self.prochaine.duration_since(maintenant), Ok(reste) if !reste.is_zero() && reste <= plus_longue)
    }

    /// Après une lecture finie à `maintenant` : quand lire la suivante, et les lignes du journal — la première lecture,
    /// une page ou Render en panne (une seule ligne jusqu'au retour), le retour, un service dont le niveau change, des
    /// erreurs de Render qui viennent ou s'en vont de la dernière heure. Une lecture est manquée quand Render ne répond
    /// pas (sa clé refusée mise à part : un nouvel essai tout de suite n'y changerait rien), ou aucune page.
    fn apres(&mut self, l: &Lecture, maintenant: SystemTime) -> Vec<String> {
        let mut lignes = Vec::new();
        let r = &l.releve;
        if !self.deja_lu {
            lignes.push(format!(
                "Services : {} (lus en {}, {} requêtes ; relus toutes les 5 min).",
                r.resume(),
                entreprise::secondes(l.duree),
                l.requetes
            ));
        }
        for (i, statut) in r.statuts.iter().enumerate() {
            let manquee = l.manquees.get(i).cloned().flatten();
            let en_panne = self.pages_en_panne.get_mut(i);
            match (manquee, en_panne) {
                (Some(e), Some(dite)) if !*dite => {
                    lignes.push(format!(
                        "Services : la page d'état de {} ne se lit pas ({e}) ; sa dernière lecture reste une heure.",
                        statut.nom
                    ));
                    *dite = true;
                }
                (None, Some(dite)) if *dite => {
                    lignes.push(format!("Services : la page d'état de {} se lit de nouveau.", statut.nom));
                    *dite = false;
                }
                _ => {}
            }
            if let (Ok(e), Some(avant)) = (&statut.etat, self.niveaux.get_mut(i)) {
                if self.deja_lu && avant.is_some_and(|n| n != e.niveau) {
                    lignes.push(match e.niveau {
                        Niveau::Operationnel => format!("Services : {} — opérationnel de nouveau.", statut.nom),
                        _ => format!("Services : {} — {}.", statut.nom, etat_en_mots(e)),
                    });
                }
                *avant = Some(e.niveau);
            }
        }
        let attente = if l.cle_refusee { PERIODE } else { self.reprise };
        match &r.render {
            Err(e) => {
                if !self.render_en_panne {
                    let quand = if l.cle_refusee {
                        "nouvel essai dans 5 min, ou dès que la clé change".to_string()
                    } else {
                        format!("nouvel essai dans {} min", attente.as_secs().div_ceil(60))
                    };
                    lignes.push(format!(
                        "Services : les erreurs de Render ne se lisent pas : {e} ; {quand} (une seule ligne \
                         jusqu'au retour)."
                    ));
                    self.render_en_panne = true;
                }
            }
            Ok(liste) => {
                if self.render_en_panne {
                    lignes.push("Services : les erreurs de Render se lisent de nouveau.".to_string());
                    self.render_en_panne = false;
                }
                let erreurs: usize = liste.iter().map(|e| e.derniere_heure).sum();
                let reponses: usize = liste.iter().map(|e| e.reponses_5xx.derniere_heure).sum();
                let recentes = erreurs + reponses > 0;
                let touchees = liste.iter().filter(|e| e.derniere_heure + e.reponses_5xx.derniere_heure > 0).count();
                if self.deja_lu && recentes && !self.erreurs_recentes {
                    lignes.push(format!(
                        "Services : sur Render, {} dans la dernière heure ({}).",
                        erreurs_en_mots(erreurs, reponses),
                        nombre(touchees, "ressource", "ressources")
                    ));
                } else if self.deja_lu && !recentes && self.erreurs_recentes {
                    lignes.push("Services : sur Render, plus d'erreur dans la dernière heure.".to_string());
                }
                self.erreurs_recentes = recentes;
            }
        }
        let manquee = (r.render.is_err() && !l.cle_refusee) || l.manquees.iter().all(Option::is_some);
        if manquee {
            self.prochaine = maintenant + self.reprise;
            self.reprise = (self.reprise * 2).min(REPRISE_MAX);
        } else {
            self.prochaine = maintenant + PERIODE;
            self.reprise = REPRISE;
        }
        self.deja_lu = true;
        lignes
    }
}

/// Un état qui ne va pas, en mots, pour le journal : « en panne partielle, 1 incident en cours, 2 composants touchés ».
fn etat_en_mots(e: &Etat) -> String {
    let mut morceaux = vec![e.niveau.libelle().to_string()];
    if !e.incidents.is_empty() {
        morceaux.push(format!("{} en cours", nombre(e.incidents.len(), "incident", "incidents")));
    }
    if !e.composants.is_empty() {
        morceaux.push(nombre(e.composants.len(), "composant touché", "composants touchés"));
    }
    morceaux.join(", ")
}

/// La dernière lecture partie, tous suivis confondus.
static DERNIERE_LECTURE: Mutex<Option<Instant>> = Mutex::new(None);

/// Prend la place d'une lecture, ou dit combien de temps attendre encore (une par minute au plus).
fn reserver() -> Result<(), Duration> {
    let mut derniere = DERNIERE_LECTURE.lock().unwrap_or_else(PoisonError::into_inner);
    let maintenant = Instant::now();
    match reste_avant(*derniere, maintenant) {
        Some(reste) => Err(reste),
        None => {
            *derniere = Some(maintenant);
            Ok(())
        }
    }
}

/// Ce qu'il faut encore attendre avant une lecture, si la dernière est partie il y a moins d'une minute.
fn reste_avant(derniere: Option<Instant>, maintenant: Instant) -> Option<Duration> {
    let ecoule = maintenant.saturating_duration_since(derniere?);
    ENTRE_DEUX_LECTURES.checked_sub(ecoule).filter(|r| !r.is_zero())
}

// ---- Une lecture ------------------------------------------------------------------------------------------------

/// Ce qu'une lecture a donné : le relevé ; et, pour le journal et le suivi, les pages qui n'ont pas répondu cette fois
/// (pourquoi : leur lecture d'avant peut rester), si Render a refusé la clé, le nombre de requêtes, la durée.
struct Lecture {
    releve: Releve,
    manquees: Vec<Option<String>>,
    cle_refusee: bool,
    requetes: usize,
    duree: Duration,
}

/// Pourquoi Render n'a rien rendu.
#[derive(Clone, Debug, PartialEq)]
enum Manque {
    /// La clé est refusée (401), ou n'a pas droit à la lecture (403) : un nouvel essai n'y changerait rien.
    Cle(String),
    /// Render ne répond pas, ou pas comme il faut : un nouvel essai, bientôt.
    Panne(String),
}

impl Manque {
    fn texte(&self) -> &str {
        match self {
            Manque::Cle(t) | Manque::Panne(t) => t,
        }
    }
}

/// Une lecture, tout de suite, avec cette clé : les pages par `get_public`, Render par [`obtenir_de_render`].
fn lire_maintenant(cle: &str, memoire: &mut Memoire) -> Lecture {
    let maintenant = unix(SystemTime::now());
    lire(maintenant, memoire, &mut |url| https::get_public(url, DELAI), &mut |route, parametres| {
        obtenir_de_render(cle, route, parametres)
    })
}

/// Une lecture à l'heure `maintenant` (en secondes Unix) : chaque page d'état par `page` (son code HTTP et son corps),
/// puis Render par `render` (une route de [`ROUTES_LUES`] et ses paramètres ; son JSON). À part pour les essais, qui
/// leur font rendre des réponses inventées.
fn lire(
    maintenant: i64,
    memoire: &mut Memoire,
    page: &mut dyn FnMut(&str) -> Result<(u16, Vec<u8>), String>,
    render: &mut dyn FnMut(&str, &[(&str, String)]) -> Result<Value, Manque>,
) -> Lecture {
    let debut = Instant::now();
    let mut requetes = 0;
    let mut statuts = Vec::with_capacity(PAGES.len());
    let mut manquees = Vec::with_capacity(PAGES.len());
    for (i, (nom, adresse, resume)) in PAGES.iter().enumerate() {
        requetes += 1;
        let lu = page(resume)
            .map_err(|e| format!("la page d'état ne répond pas : {e}"))
            .and_then(|(code, corps)| etat_de_la_reponse(code, &corps, adresse));
        let gardee = memoire.get(i).cloned().flatten();
        let etat = match lu {
            Ok(e) => {
                if let Some(m) = memoire.get_mut(i) {
                    *m = Some((e.clone(), maintenant));
                }
                manquees.push(None);
                Ok(e)
            }
            Err(raison) => {
                manquees.push(Some(raison.clone()));
                match gardee {
                    Some((e, lu_a)) if maintenant - lu_a <= VALIDITE.as_secs() as i64 => Ok(e),
                    _ => Err(raison),
                }
            }
        };
        statuts.push(Statut { nom: (*nom).to_string(), page: (*adresse).to_string(), etat });
    }
    let mut compter = |route: &str, parametres: &[(&str, String)]| {
        requetes += 1;
        render(route, parametres)
    };
    let lu = lire_render(maintenant, &mut compter);
    let cle_refusee = matches!(lu, Err(Manque::Cle(_)));
    let releve = Releve { lu_a: maintenant, statuts, render: lu.map_err(|m| m.texte().to_string()) };
    Lecture { releve, manquees, cle_refusee, requetes, duree: debut.elapsed() }
}

/// L'heure en secondes Unix.
fn unix(t: SystemTime) -> i64 {
    t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

// ---- Les pages d'état -------------------------------------------------------------------------------------------

/// L'état d'une réponse d'une page, selon son code HTTP (`page` : l'adresse de la page, pour les liens).
fn etat_de_la_reponse(code: u16, corps: &[u8], page: &str) -> Result<Etat, String> {
    match code {
        200 => etat_de(corps, page),
        300..=399 => Err(format!("la page d'état renvoie ailleurs (HTTP {code})")),
        _ => Err(format!("la page d'état répond HTTP {code}")),
    }
}

/// L'état d'un service d'après le résumé de sa page (Statuspage, `/api/v2/summary.json`) : l'indicateur de la page
/// (`status.indicator`) ; ses incidents non résolus, et ses maintenances en cours (pas celles qui viennent) ; ses
/// composants qui ne vont pas bien. Le niveau est le pire des trois : un composant en panne sous un indicateur resté à
/// « none » se voit quand même. `page` : l'adresse de la page, le lien d'un incident qui n'a pas le sien.
pub fn etat_de(json: &[u8], page: &str) -> Result<Etat, String> {
    let v: Value = serde_json::from_slice(json).map_err(|_| "la page d'état ne rend pas du JSON".to_string())?;
    let indicateur = v.pointer("/status/indicator").and_then(Value::as_str);
    if indicateur.is_none() && !v["components"].is_array() {
        return Err("ce n'est pas le résumé d'une page d'état".into());
    }
    let mut incidents: Vec<Incident> = v["incidents"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|i| incident_de(i, page, false))
        .chain(v["scheduled_maintenances"].as_array().into_iter().flatten().filter_map(|m| incident_de(m, page, true)))
        .collect();
    incidents.sort_by(|a, b| b.impact.cmp(&a.impact).then(b.depuis.cmp(&a.depuis)));
    incidents.truncate(INCIDENTS_MAX);
    let composants = composants_de(&v);
    let niveau = indicateur
        .and_then(Niveau::de)
        .into_iter()
        .chain(incidents.iter().map(|i| i.impact))
        .chain(composants.iter().map(|c| c.1))
        .max()
        .unwrap_or(Niveau::Operationnel);
    Ok(Etat { niveau, incidents, composants })
}

/// Un incident d'une page, s'il est en cours : un incident pas encore résolu (« investigating », « identified »,
/// « monitoring »), ou une maintenance commencée (« in_progress », « verifying ») — `maintenance` : il vient de
/// `scheduled_maintenances`. Son impact (un incident d'impact inconnu compte pour mineur), depuis quand (son début,
/// sinon l'heure prévue d'une maintenance, sinon sa création), son lien.
fn incident_de(i: &Value, page: &str, maintenance: bool) -> Option<Incident> {
    let statut = i["status"].as_str().unwrap_or_default().trim();
    let en_cours = if maintenance {
        matches!(statut, "in_progress" | "verifying")
    } else {
        !matches!(statut, "resolved" | "postmortem" | "completed")
    };
    if !en_cours {
        return None;
    }
    let d_office = if maintenance { Niveau::Maintenance } else { Niveau::Mineur };
    let impact = i["impact"].as_str().and_then(Niveau::de).unwrap_or(d_office);
    let nom = ecourter_a(&ligne_propre(i["name"].as_str().unwrap_or_default()), NOM_MAX);
    let nom = if nom.is_empty() { if maintenance { "Maintenance" } else { "Incident" }.to_string() } else { nom };
    let depuis = ["started_at", "scheduled_for", "created_at"]
        .iter()
        .find_map(|champ| i[*champ].as_str().and_then(instant_de))
        .unwrap_or(0);
    Some(Incident { nom, impact, statut: ecourter_a(&ligne_propre(statut), 40), depuis, lien: lien_de(i, page) })
}

/// Le lien d'un incident : son lien court (`shortlink`), s'il est en https ; sinon sa page sur la page d'état, sinon
/// la page d'état elle-même.
fn lien_de(i: &Value, page: &str) -> String {
    let lien = i["shortlink"].as_str().map(str::trim).filter(|l| lien_valable(l));
    if let Some(l) = lien {
        return l.to_string();
    }
    match i["id"].as_str().filter(|id| identifiant_valable(id)) {
        Some(id) => format!("{page}/incidents/{id}"),
        None => page.to_string(),
    }
}

/// Un lien qui s'ouvre d'un clic : en https, des caractères visibles, court.
fn lien_valable(l: &str) -> bool {
    l.len() <= 200 && l.starts_with("https://") && l.len() > 8 && l.bytes().all(|b| b.is_ascii_graphic())
}

/// Les composants d'une page qui ne vont pas bien, le pire d'abord, puis dans l'ordre de la page. Un groupe (Render en
/// a un par région, aux composants de mêmes noms) ne compte pas lui-même : ses composants portent son nom entre
/// parenthèses, « Web Services (Oregon) ». Un état inconnu ne se dit pas.
fn composants_de(v: &Value) -> Vec<(String, Niveau)> {
    let tous = v["components"].as_array().map(Vec::as_slice).unwrap_or_default();
    let groupes: HashMap<&str, &str> = tous
        .iter()
        .filter(|c| c["group"].as_bool() == Some(true))
        .filter_map(|c| Some((c["id"].as_str()?, c["name"].as_str()?)))
        .collect();
    let mut touches: Vec<(Niveau, usize, String)> = tous
        .iter()
        .enumerate()
        .filter(|(_, c)| c["group"].as_bool() != Some(true))
        .filter_map(|(rang, c)| {
            let niveau = c["status"].as_str().and_then(Niveau::de).filter(|n| *n != Niveau::Operationnel)?;
            let nom = ligne_propre(c["name"].as_str().unwrap_or_default());
            let groupe = c["group_id"].as_str().and_then(|g| groupes.get(g)).map(|g| ligne_propre(g));
            let nom = match groupe {
                Some(g) if !g.is_empty() => format!("{nom} ({g})"),
                _ => nom,
            };
            Some((niveau, rang, ecourter_a(&nom, NOM_MAX)))
        })
        .collect();
    touches.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    touches.into_iter().take(COMPOSANTS_MAX).map(|(niveau, _, nom)| (nom, niveau)).collect()
}

// ---- Render -----------------------------------------------------------------------------------------------------

/// Une ressource de Render dont Eve lit le journal : son identifiant (« srv-… », « dpg-… »), son équipe (que demande
/// la lecture des journaux), son nom, sa sorte.
#[derive(Clone, Debug, PartialEq)]
struct Ressource {
    id: String,
    equipe: String,
    nom: String,
    sorte: Sorte,
}

/// La seule requête qu'Eve fait à Render : un GET — le verbe est écrit ici, jamais pris d'ailleurs —, sur une route de
/// [`ROUTES_LUES`], la clé en « Bearer » ; rend le JSON de la réponse. Une requête qui n'aboutit pas (le délai passé,
/// la connexion coupée) se refait une fois ; une réponse, quelle qu'elle soit, jamais.
fn obtenir_de_render(cle: &str, route: &str, parametres: &[(&str, String)]) -> Result<Value, Manque> {
    let url = adresse_de_render(route, parametres).map_err(Manque::Panne)?;
    let r = https::requete("GET", &url, cle, &[], None, DELAI_RENDER)
        .or_else(|_| https::requete("GET", &url, cle, &[], None, DELAI_RENDER))
        .map_err(|e| Manque::Panne(format!("l'API de Render ne répond pas : {e}")))?;
    reponse_de_render(r.statut, &r.corps)
}

/// L'adresse d'une lecture de Render : son API, une route de [`ROUTES_LUES`], ses paramètres encodés. Une autre route
/// est refusée, avant tout envoi.
fn adresse_de_render(route: &str, parametres: &[(&str, String)]) -> Result<String, String> {
    if !ROUTES_LUES.contains(&route) {
        return Err(format!("la route {route} n'est pas une lecture permise à Eve : rien n'est envoyé"));
    }
    let requete: Vec<String> =
        parametres.iter().map(|(n, v)| format!("{}={}", https::encoder(n), https::encoder(v))).collect();
    Ok(if requete.is_empty() { format!("{API}{route}") } else { format!("{API}{route}?{}", requete.join("&")) })
}

/// Le JSON d'une réponse de Render, selon son code HTTP ; un refus dit sa raison quand Render la donne
/// (`{"message": "…"}`).
fn reponse_de_render(statut: u16, corps: &[u8]) -> Result<Value, Manque> {
    match statut {
        200 => {
            serde_json::from_slice(corps).map_err(|_| Manque::Panne("la réponse de Render n'est pas du JSON".into()))
        }
        401 => Err(Manque::Cle(CLE_REFUSEE.into())),
        403 => Err(Manque::Cle(ACCES_REFUSE.into())),
        429 => Err(Manque::Panne("Render demande d'attendre (HTTP 429)".into())),
        500..=599 => Err(Manque::Panne(format!("l'API de Render ne répond pas (HTTP {statut})"))),
        s => {
            let message =
                serde_json::from_slice::<Value>(corps).ok().and_then(|v| v["message"].as_str().map(ligne_propre));
            let dite = message.filter(|m| !m.is_empty()).map(|m| format!(" : {}", ecourter_a(&m, 160)));
            Err(Manque::Panne(format!("Render répond HTTP {s}{}", dite.unwrap_or_default())))
        }
    }
}

/// Une lecture de Render à l'heure `maintenant` : ses services et ses bases, puis le journal de chacun sur la
/// [`FENETRE`] — ses erreurs, et pour un service web ses réponses 5xx. `obtenir` fait chaque requête
/// ([`obtenir_de_render`], ou les réponses inventées des essais). Une requête qui manque fait manquer la lecture : le
/// relevé ne mêle jamais une lecture d'avant à celle-ci.
fn lire_render(
    maintenant: i64,
    obtenir: &mut dyn FnMut(&str, &[(&str, String)]) -> Result<Value, Manque>,
) -> Result<Vec<Erreurs>, Manque> {
    let liste = [("limit", RESSOURCES_PAR_LISTE.to_string())];
    let services = obtenir("/v1/services", &liste)?;
    let bases = obtenir("/v1/postgres", &liste)?;
    let ressources = ressources_de(&services, &bases).map_err(Manque::Panne)?;
    let debut = maintenant - FENETRE.as_secs() as i64;
    ressources
        .iter()
        .map(|r| {
            let journal = obtenir("/v1/logs", &parametres_des_erreurs(r, debut, maintenant))?;
            let mut e = erreurs_de(&journal, r, maintenant).map_err(Manque::Panne)?;
            if r.sorte == Sorte::Web {
                let requetes = obtenir("/v1/logs", &parametres_des_5xx(r, debut, maintenant))?;
                e.reponses_5xx = reponses_5xx_de(&requetes, maintenant).map_err(Manque::Panne)?;
            }
            Ok(e)
        })
        .collect()
}

/// Les ressources de Render dont Eve lit le journal, d'après la liste des services (`[{"service": {…}}]`) et celle des
/// bases (`[{"postgres": {…}}]`) : les services web d'abord, puis les bases, puis les autres services, chacun par son
/// nom. Sautés : les suspendus (rien n'y tourne), les sites statiques (Render les sert sans journal), une ressource à
/// l'identifiant douteux (il irait dans une requête).
fn ressources_de(services: &Value, bases: &Value) -> Result<Vec<Ressource>, String> {
    let services = services.as_array().ok_or("la liste des services de Render ne se lit pas")?;
    let bases = bases.as_array().ok_or("la liste des bases de Render ne se lit pas")?;
    let mut ressources = Vec::new();
    for s in services.iter().map(|e| &e["service"]) {
        if s["suspended"] == "suspended" || s["type"] == "static_site" {
            continue;
        }
        let sorte = if s["type"] == "web_service" { Sorte::Web } else { Sorte::Autre };
        ressources.extend(ressource(&s["id"], &s["ownerId"], &s["name"], sorte));
    }
    for b in bases.iter().map(|e| &e["postgres"]) {
        if b["suspended"] == "suspended" || b["status"] == "suspended" {
            continue;
        }
        ressources.extend(ressource(&b["id"], &b["owner"]["id"], &b["name"], Sorte::Base));
    }
    let rang = |s: Sorte| match s {
        Sorte::Web => 0,
        Sorte::Base => 1,
        Sorte::Autre => 2,
    };
    ressources.sort_by(|a, b| {
        rang(a.sorte).cmp(&rang(b.sorte)).then_with(|| a.nom.to_lowercase().cmp(&b.nom.to_lowercase()))
    });
    Ok(ressources)
}

/// Une ressource, si son identifiant et son équipe ont la forme de ceux de Render ; son nom ébarbé (sans nom : son
/// identifiant).
fn ressource(id: &Value, equipe: &Value, nom: &Value, sorte: Sorte) -> Option<Ressource> {
    let id = id.as_str().filter(|i| identifiant_valable(i))?.to_string();
    let equipe = equipe.as_str().filter(|e| identifiant_valable(e))?.to_string();
    let nom = ecourter_a(&ligne_propre(nom.as_str().unwrap_or_default()), NOM_MAX);
    let nom = if nom.is_empty() { id.clone() } else { nom };
    Some(Ressource { id, equipe, nom, sorte })
}

/// Un identifiant de Render ou de Statuspage : des lettres, des chiffres, « - » et « _ », court.
fn identifiant_valable(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Les paramètres de la lecture d'un journal : ce `filtre` (une étiquette et sa valeur), de `debut` à `fin` (en
/// secondes Unix), les lignes les plus récentes d'abord, cent au plus.
fn parametres(r: &Ressource, filtre: &[(&'static str, &str)], debut: i64, fin: i64) -> Vec<(&'static str, String)> {
    let mut p = vec![("ownerId", r.equipe.clone()), ("resource", r.id.clone())];
    p.extend(filtre.iter().map(|(n, v)| (*n, (*v).to_string())));
    p.extend([
        ("startTime", rfc3339(debut)),
        ("endTime", rfc3339(fin)),
        ("direction", "backward".into()),
        ("limit", LIGNES_PAR_LECTURE.to_string()),
    ]);
    p
}

/// Les erreurs d'une ressource : les lignes de son journal au niveau « error ».
fn parametres_des_erreurs(r: &Ressource, debut: i64, fin: i64) -> Vec<(&'static str, String)> {
    parametres(r, &[("level", "error")], debut, fin)
}

/// Les réponses 5xx d'un service web : ses lignes de requête dont le code porte un 5 (le filtre de Render cherche un
/// morceau du code : [`reponses_5xx_de`] garde les 500 à 599).
fn parametres_des_5xx(r: &Ressource, debut: i64, fin: i64) -> Vec<(&'static str, String)> {
    parametres(r, &[("type", "request"), ("statusCode", "5*")], debut, fin)
}

/// Ce que compte la lecture d'un journal : combien, s'il y en a davantage, combien dans la dernière heure, la plus
/// récente — la forme de [`Reponses5xx`], que les erreurs reprennent champ par champ.
type Compte = Reponses5xx;

/// Ce que comptent les lignes d'une réponse de `/v1/logs` (`{"hasMore": …, "logs": [{"timestamp": …, "message": …,
/// "labels": […]}]}`) que `garder` retient : combien, s'il y en a davantage (`hasMore`), combien dans la dernière heure
/// avant `maintenant`, la plus récente — Render rend les plus récentes, mais dans l'ordre du temps : elle se cherche.
fn compter(v: &Value, garder: impl Fn(&Value) -> bool, maintenant: i64) -> Result<Compte, String> {
    let lignes: Vec<&Value> =
        v["logs"].as_array().ok_or("le journal de Render ne se lit pas")?.iter().filter(|l| garder(l)).collect();
    let quand = |l: &Value| l["timestamp"].as_str().and_then(instant_de);
    let derniere = lignes
        .iter()
        .max_by_key(|l| quand(l).unwrap_or(i64::MIN))
        .map(|l| Ligne { quand: quand(l).unwrap_or(0), message: message_de(l) });
    Ok(Compte {
        nombre: lignes.len(),
        plus: v["hasMore"].as_bool() == Some(true),
        derniere_heure: lignes.iter().filter(|l| quand(l).is_some_and(|t| t >= maintenant - 3600)).count(),
        derniere,
    })
}

/// Les erreurs d'une ressource d'après la lecture de son journal au niveau « error » (ses réponses 5xx viennent à
/// part : [`reponses_5xx_de`]).
fn erreurs_de(v: &Value, r: &Ressource, maintenant: i64) -> Result<Erreurs, String> {
    let c = compter(v, |_| true, maintenant)?;
    Ok(Erreurs {
        nom: r.nom.clone(),
        sorte: r.sorte,
        nombre: c.nombre,
        plus: c.plus,
        derniere_heure: c.derniere_heure,
        derniere: c.derniere,
        reponses_5xx: Reponses5xx::default(),
    })
}

/// Les réponses 5xx d'un service web d'après la lecture de ses lignes de requête : celles d'un code de 500 à 599 qui
/// ne sont pas au niveau « error » (celles-là comptent parmi ses erreurs).
fn reponses_5xx_de(v: &Value, maintenant: i64) -> Result<Reponses5xx, String> {
    let garder = |l: &Value| {
        let code = etiquette(l, "statusCode").and_then(|c| c.parse::<u16>().ok());
        code.is_some_and(|c| (500..=599).contains(&c)) && etiquette(l, "level") != Some("error")
    };
    compter(v, garder, maintenant)
}

/// La valeur d'une étiquette d'une ligne d'un journal de Render (`"labels": [{"name": …, "value": …}]`).
fn etiquette<'a>(l: &'a Value, nom: &str) -> Option<&'a str> {
    l["labels"].as_array().into_iter().flatten().find(|e| e["name"] == nom).and_then(|e| e["value"].as_str())
}

/// Le message d'une ligne d'un journal de Render, sur une ligne. Une ligne de requête (`type=request`) se dit par ses
/// étiquettes — « HTTP 500 · POST /api » — plutôt que par son texte, qui porte l'adresse IP et le navigateur de qui a
/// fait la requête. Un mot qui porte une clé ou un jeton ne s'affiche pas.
fn message_de(l: &Value) -> String {
    let requete = (etiquette(l, "type") == Some("request"))
        .then(|| {
            let code = etiquette(l, "statusCode").map(|c| format!("HTTP {c}"));
            let quoi = [etiquette(l, "method"), etiquette(l, "path")].into_iter().flatten().collect::<Vec<_>>();
            let quoi = quoi.join(" ");
            [code.unwrap_or_default(), quoi].into_iter().filter(|m| !m.is_empty()).collect::<Vec<_>>().join(" · ")
        })
        .filter(|m| !m.is_empty());
    let texte = requete.unwrap_or_else(|| l["message"].as_str().unwrap_or_default().to_string());
    ecourter_a(&masquer(sans_prefixe_postgres(&ligne_propre(&texte))), MESSAGE_MAX)
}

/// Une ligne du journal d'une base Postgres de Render commence par son préfixe — « [6adf7730.fd5e5-4]
/// user=…,db=…,app=…,client=…, » : qui, d'où —, puis sa gravité et ce qui a eu lieu (« ERROR:  canceling statement due
/// to statement timeout ») : elle se montre à partir de sa gravité. Vu le 27 octobre 2026 : coupé à sa longueur, le
/// préfixe seul se voyait. Une autre ligne reste telle quelle.
fn sans_prefixe_postgres(t: &str) -> &str {
    if !t.starts_with('[') || !t.contains(" user=") {
        return t;
    }
    ["PANIC:", "FATAL:", "ERROR:", "WARNING:", "LOG:", "DETAIL:", "STATEMENT:"]
        .iter()
        .filter_map(|g| t.find(g))
        .min()
        .map_or(t, |i| &t[i..])
}

// ---- Les heures et les textes -----------------------------------------------------------------------------------

/// Un instant Unix en RFC 3339, en temps universel, à la seconde : « 2026-10-26T12:34:32Z » (les bornes d'une lecture
/// des journaux de Render).
fn rfc3339(unix: i64) -> String {
    let (jour, s) = (unix.div_euclid(86_400), unix.rem_euclid(86_400));
    format!("{}T{:02}:{:02}:{:02}Z", centre::date_du_numero(jour), s / 3600, s / 60 % 60, s % 60)
}

/// L'instant Unix d'une heure RFC 3339 de Render ou d'une page d'état (« 2026-10-27T12:23:04.496878012Z »,
/// « 2026-10-27T08:15:00.123-04:00 ») : ses fractions de seconde ôtées, elle se lit comme toute heure ISO 8601 d'Eve
/// (`entreprise::en_unix`).
fn instant_de(iso: &str) -> Option<i64> {
    let iso = iso.trim();
    let (debut, reste) = (iso.get(..19)?, iso.get(19..)?);
    let reste = reste.strip_prefix('.').map_or(reste, |f| f.trim_start_matches(|c: char| c.is_ascii_digit()));
    entreprise::en_unix(&format!("{debut}{reste}"))
}

/// La première ligne non vide d'un texte venu d'ailleurs (un nom, un message de journal) : les couleurs des terminaux
/// (« ESC [ 31 m ») et les caractères de contrôle ôtés, une tabulation devenue une espace, ses blancs ébarbés.
fn ligne_propre(t: &str) -> String {
    for ligne in t.split(['\n', '\r']) {
        let mut propre = String::with_capacity(ligne.len());
        let mut c = ligne.chars().peekable();
        while let Some(x) = c.next() {
            match x {
                '\u{1b}' => {
                    // Une séquence de commande (CSI) : « [ », ses paramètres, puis sa lettre finale, de « @ » à « ~ ».
                    if c.next_if_eq(&'[').is_some() {
                        for y in c.by_ref() {
                            if ('@'..='~').contains(&y) {
                                break;
                            }
                        }
                    } else {
                        c.next();
                    }
                }
                '\t' => propre.push(' '),
                x if x.is_control() => {}
                x => propre.push(x),
            }
        }
        let propre = propre.trim();
        if !propre.is_empty() {
            return propre.to_string();
        }
    }
    String::new()
}

/// Un texte de `max` caractères au plus : au-delà, coupé, son dernier caractère devenu « … ».
fn ecourter_a(t: &str, max: usize) -> String {
    if t.chars().count() <= max {
        return t.to_string();
    }
    let mut s: String = t.chars().take(max.saturating_sub(1)).collect();
    s.push('…');
    s
}

/// Un texte sans clé ni jeton : ce qui suit « Bearer » et les jetons JWT (`connexion::sans_porteur`), et tout mot qui
/// porte le début d'une clé ([`SECRETS`]), deviennent « … ».
fn masquer(t: &str) -> String {
    connexion::sans_porteur(t)
        .split(' ')
        .map(|m| if SECRETS.iter().any(|s| m.contains(s)) { "…" } else { m })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod essais {
    use serde_json::json;

    use super::*;

    // Les réponses sont de la forme de celles des pages d'état et de l'API de Render (relevées le 27 octobre 2026) ;
    // leurs valeurs sont inventées.

    /// Une heure quelconque : le 27 octobre 2026, 12 h 34 min 32 s, en temps universel.
    const MAINTENANT: i64 = 1_793_104_472;

    fn instant() -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(MAINTENANT as u64)
    }

    /// Le résumé d'une page où tout va bien.
    fn page_en_ordre() -> Value {
        json!({
            "page": {"id": "pg01exemple", "name": "Exemple", "url": "https://status.exemple.com",
                     "time_zone": "Etc/UTC", "updated_at": "2026-10-27T12:29:47.460Z"},
            "components": [
                {"id": "c01", "name": "Tableau de bord", "status": "operational", "position": 1, "group": false,
                 "group_id": null, "showcase": true, "only_show_if_degraded": false},
                {"id": "c02", "name": "API", "status": "operational", "position": 2, "group": false,
                 "group_id": null, "showcase": true, "only_show_if_degraded": false}
            ],
            "incidents": [],
            "scheduled_maintenances": [],
            "status": {"indicator": "none", "description": "All Systems Operational"}
        })
    }

    /// Une page qui ne va pas : un incident en cours, un autre résolu, une maintenance commencée, une autre à venir ;
    /// un groupe par région (comme celle de Render), un composant perturbé dans l'un, un autre en panne hors groupe.
    fn page_en_panne() -> Value {
        json!({
            "page": {"id": "pg02exemple", "name": "Exemple", "url": "https://status.exemple.com"},
            "components": [
                {"id": "c01", "name": "Tableau de bord", "status": "operational", "group": false, "group_id": null},
                {"id": "c10", "name": "Services web", "status": "degraded_performance", "group": false,
                 "group_id": "g01"},
                {"id": "c11", "name": "Services web", "status": "operational", "group": false, "group_id": "g02"},
                {"id": "c02", "name": " API\tpublique ", "status": "partial_outage", "group": false, "group_id": null},
                {"id": "c03", "name": "Courriels", "status": "statut_inconnu", "group": false, "group_id": null},
                {"id": "g01", "name": "Région Est", "status": "degraded_performance", "group": true,
                 "components": ["c10"]},
                {"id": "g02", "name": "Région Ouest", "status": "operational", "group": true, "components": ["c11"]}
            ],
            "incidents": [
                {"id": "inc01exemple", "name": "Erreurs en hausse sur l'API", "status": "investigating",
                 "impact": "major", "created_at": "2026-10-27T11:58:00.120Z", "started_at": "2026-10-27T11:55:00Z",
                 "shortlink": "https://stspg.io/exemple1", "incident_updates": []},
                {"id": "inc02exemple", "name": "Lenteur réglée", "status": "resolved", "impact": "critical",
                 "created_at": "2026-10-27T09:00:00Z", "shortlink": "https://stspg.io/exemple2"},
                {"id": "inc03exemple", "name": "Connexions lentes", "status": "monitoring", "impact": "minor",
                 "created_at": "2026-10-27T08:00:00.000-04:00", "shortlink": "http://stspg.io/exemple3"}
            ],
            "scheduled_maintenances": [
                {"id": "mnt01exemple", "name": "Mise à niveau des bases", "status": "in_progress",
                 "impact": "maintenance", "scheduled_for": "2026-10-27T12:00:00.000Z",
                 "scheduled_until": "2026-10-27T14:00:00.000Z", "shortlink": "https://stspg.io/exemple4"},
                {"id": "mnt02exemple", "name": "Maintenance de demain", "status": "scheduled",
                 "impact": "maintenance", "scheduled_for": "2026-10-28T12:00:00.000Z"}
            ],
            "status": {"indicator": "major", "description": "Partial System Outage"}
        })
    }

    fn octets(v: &Value) -> Vec<u8> {
        serde_json::to_vec(v).unwrap()
    }

    #[test]
    fn les_mots_de_statuspage() {
        let cas = [
            ("none", Niveau::Operationnel),
            ("operational", Niveau::Operationnel),
            ("maintenance", Niveau::Maintenance),
            ("under_maintenance", Niveau::Maintenance),
            ("minor", Niveau::Mineur),
            ("degraded_performance", Niveau::Mineur),
            ("major", Niveau::Majeur),
            ("partial_outage", Niveau::Majeur),
            ("critical", Niveau::Critique),
            ("major_outage", Niveau::Critique),
        ];
        for (mot, niveau) in cas {
            assert_eq!(Niveau::de(mot), Some(niveau), "{mot}");
        }
        assert_eq!(Niveau::de("degraded"), None);
        assert_eq!(Niveau::de(""), None);
        // Du meilleur au pire : le pire de plusieurs se prend par `max`.
        assert!(Niveau::Operationnel < Niveau::Maintenance && Niveau::Maintenance < Niveau::Mineur);
        assert!(Niveau::Mineur < Niveau::Majeur && Niveau::Majeur < Niveau::Critique);
        assert_eq!(Niveau::Majeur.libelle(), "en panne partielle");
    }

    #[test]
    fn une_page_en_ordre() {
        let e = etat_de(&octets(&page_en_ordre()), "https://status.exemple.com").unwrap();
        assert_eq!(e, Etat { niveau: Niveau::Operationnel, incidents: vec![], composants: vec![] });
    }

    #[test]
    fn une_page_en_panne() {
        let e = etat_de(&octets(&page_en_panne()), "https://status.exemple.com").unwrap();
        assert_eq!(e.niveau, Niveau::Majeur);
        // L'incident résolu et la maintenance à venir n'y sont pas ; le plus grave d'abord.
        let noms: Vec<&str> = e.incidents.iter().map(|i| i.nom.as_str()).collect();
        assert_eq!(noms, ["Erreurs en hausse sur l'API", "Connexions lentes", "Mise à niveau des bases"]);
        let premier = &e.incidents[0];
        assert_eq!((premier.impact, premier.statut.as_str()), (Niveau::Majeur, "investigating"));
        assert_eq!(premier.depuis, MAINTENANT - 39 * 60 - 32, "son début, pas sa création");
        assert_eq!(premier.lien, "https://stspg.io/exemple1");
        // Un lien court en http : la page de l'incident sur la page d'état ; une heure avec son décalage.
        let second = &e.incidents[1];
        assert_eq!(second.lien, "https://status.exemple.com/incidents/inc03exemple");
        assert_eq!(second.depuis, MAINTENANT - 34 * 60 - 32);
        // La maintenance commencée : depuis l'heure prévue, faute de début.
        let maintenance = &e.incidents[2];
        assert_eq!((maintenance.impact, maintenance.statut.as_str()), (Niveau::Maintenance, "in_progress"));
        assert_eq!(maintenance.depuis, MAINTENANT - 34 * 60 - 32);
        // Le pire d'abord ; un composant dans un groupe porte son nom ; le groupe lui-même, un état inconnu : non.
        let attendus =
            [("API publique".to_string(), Niveau::Majeur), ("Services web (Région Est)".into(), Niveau::Mineur)];
        assert_eq!(e.composants, attendus);
    }

    #[test]
    fn le_niveau_est_le_pire_de_la_page() {
        // L'indicateur dit « none », mais un composant est en panne : la page ne va pas.
        let mut v = page_en_ordre();
        v["components"][1]["status"] = json!("major_outage");
        let e = etat_de(&octets(&v), "https://status.exemple.com").unwrap();
        assert_eq!((e.niveau, e.composants.len()), (Niveau::Critique, 1));
        // Un indicateur inconnu ne dit rien : le reste en décide.
        let mut v = page_en_ordre();
        v["status"]["indicator"] = json!("nouveau_mot");
        assert_eq!(etat_de(&octets(&v), "https://status.exemple.com").unwrap().niveau, Niveau::Operationnel);
        // Une maintenance en cours, seule : en maintenance.
        let mut v = page_en_ordre();
        v["status"]["indicator"] = json!("maintenance");
        v["components"][0]["status"] = json!("under_maintenance");
        let e = etat_de(&octets(&v), "https://status.exemple.com").unwrap();
        assert_eq!((e.niveau, e.composants[0].1), (Niveau::Maintenance, Niveau::Maintenance));
    }

    #[test]
    fn un_incident_sans_lien_ni_nom() {
        let mut v = page_en_ordre();
        v["incidents"] = json!([
            {"id": "a b", "name": "", "status": "identified", "impact": "inconnu"},
            {"id": "inc09", "name": "\u{1b}[31mPanne\u{1b}[0m\nsuite", "status": "", "impact": "critical",
             "shortlink": "https://exemple.com/x y"}
        ]);
        let e = etat_de(&octets(&v), "https://status.exemple.com").unwrap();
        assert_eq!(e.niveau, Niveau::Critique);
        let critique = &e.incidents[0];
        assert_eq!((critique.nom.as_str(), critique.depuis), ("Panne", 0));
        assert_eq!(critique.lien, "https://status.exemple.com/incidents/inc09", "un lien court douteux ne sert pas");
        let mineur = &e.incidents[1];
        assert_eq!((mineur.nom.as_str(), mineur.impact), ("Incident", Niveau::Mineur), "un impact inconnu : mineur");
        assert_eq!(mineur.lien, "https://status.exemple.com", "ni lien ni identifiant sûr : la page");
    }

    #[test]
    fn une_page_douteuse_ne_donne_pas_d_etat() {
        let page = "https://status.exemple.com";
        assert!(etat_de(b"<html>", page).unwrap_err().contains("JSON"));
        assert!(etat_de(b"{\"page\": {}}", page).unwrap_err().contains("résumé"));
        assert!(etat_de(b"[]", page).is_err());
        assert_eq!(etat_de_la_reponse(503, b"", page).unwrap_err(), "la page d'état répond HTTP 503");
        assert_eq!(etat_de_la_reponse(301, b"", page).unwrap_err(), "la page d'état renvoie ailleurs (HTTP 301)");
        assert!(etat_de_la_reponse(200, &octets(&page_en_ordre()), page).is_ok());
    }

    #[test]
    fn les_heures_de_render() {
        assert_eq!(rfc3339(MAINTENANT), "2026-10-27T12:34:32Z");
        assert_eq!(rfc3339(MAINTENANT - FENETRE.as_secs() as i64), "2026-10-26T12:34:32Z");
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(instant_de("2026-10-27T12:34:32Z"), Some(MAINTENANT));
        assert_eq!(instant_de("2026-10-27T12:34:32.496878012Z"), Some(MAINTENANT));
        assert_eq!(instant_de("2026-10-27T12:34:32.460Z"), Some(MAINTENANT));
        assert_eq!(instant_de("2026-10-27T08:34:32.120-04:00"), Some(MAINTENANT));
        assert_eq!(instant_de(" 2026-10-27T08:34:32-04:00 "), Some(MAINTENANT));
        for douteuse in ["", "2026-10-27", "2026-10-27T12:34", "hier", "2026-10-27T12:34:32.5X"] {
            assert_eq!(instant_de(douteuse), None, "{douteuse}");
        }
        // Dans les deux sens, à travers une fin d'année.
        let t = MAINTENANT + 65 * 86_400 + 3_599;
        assert_eq!(instant_de(&rfc3339(t)), Some(t));
        assert_eq!(rfc3339(t), "2026-12-31T13:34:31Z");
    }

    /// La liste des services, de la forme de `/v1/services` : deux services web (le nom de l'un commence par une
    /// espace), un suspendu, un site statique, un travailleur, un identifiant douteux.
    fn services() -> Value {
        let s = |id: &str, nom: &str, sorte: &str, suspendu: &str| {
            json!({"cursor": "curseur", "service": {"id": id, "name": nom, "ownerId": "tea-exemple01",
                   "type": sorte, "suspended": suspendu, "suspenders": [], "slug": "exemple",
                   "serviceDetails": {"region": "oregon", "plan": "starter"}}})
        };
        json!([
            s("srv-exemple02", "app-exemple", "web_service", "not_suspended"),
            s("srv-exemple01", " api-exemple", "web_service", "not_suspended"),
            s("srv-exemple03", "ancienne-app", "web_service", "suspended"),
            s("srv-exemple04", "site-exemple", "static_site", "not_suspended"),
            s("srv-exemple05", "travailleur-exemple", "background_worker", "not_suspended"),
            s("srv-exemple06?x=1", "douteux", "web_service", "not_suspended"),
        ])
    }

    /// La liste des bases, de la forme de `/v1/postgres` : une base en marche, une suspendue.
    fn bases() -> Value {
        let b = |id: &str, nom: &str, etat: &str, suspendu: &str| {
            json!({"cursor": "curseur", "postgres": {"id": id, "name": nom, "status": etat, "suspended": suspendu,
                   "owner": {"id": "tea-exemple01", "name": "Construction Exemple inc.", "type": "team"},
                   "plan": "basic_1gb", "region": "oregon", "version": "16"}})
        };
        json!([
            b("dpg-exemple01", "Base_Exemple", "available", "not_suspended"),
            b("dpg-exemple02", "vieille", "suspended", "suspended")
        ])
    }

    #[test]
    fn les_ressources_de_render() {
        let r = ressources_de(&services(), &bases()).unwrap();
        let vues: Vec<(&str, &str, &str, Sorte)> =
            r.iter().map(|r| (r.id.as_str(), r.equipe.as_str(), r.nom.as_str(), r.sorte)).collect();
        assert_eq!(
            vues,
            [
                ("srv-exemple01", "tea-exemple01", "api-exemple", Sorte::Web),
                ("srv-exemple02", "tea-exemple01", "app-exemple", Sorte::Web),
                ("dpg-exemple01", "tea-exemple01", "Base_Exemple", Sorte::Base),
                ("srv-exemple05", "tea-exemple01", "travailleur-exemple", Sorte::Autre),
            ]
        );
        assert!(ressources_de(&json!({"erreur": 1}), &bases()).is_err());
        assert!(ressources_de(&services(), &json!(null)).is_err());
        assert_eq!(ressources_de(&json!([]), &json!([])).unwrap(), []);
    }

    /// Une ligne d'un journal de Render, de la forme de `/v1/logs`.
    fn ligne(quand: &str, message: &str, etiquettes: &[(&str, &str)]) -> Value {
        let labels: Vec<Value> = etiquettes.iter().map(|(n, v)| json!({"name": n, "value": v})).collect();
        json!({"id": "00000000-0000-0000-0000-000000000000", "timestamp": quand, "message": message, "labels": labels})
    }

    fn journal(lignes: Vec<Value>, encore: bool) -> Value {
        json!({"hasMore": encore, "logs": lignes, "nextStartTime": "2026-10-26T12:34:32Z",
               "nextEndTime": "2026-10-27T12:00:00Z"})
    }

    fn api() -> Ressource {
        Ressource {
            id: "srv-exemple01".into(),
            equipe: "tea-exemple01".into(),
            nom: "api-exemple".into(),
            sorte: Sorte::Web,
        }
    }

    #[test]
    fn les_erreurs_d_une_ressource() {
        let erreur = [("resource", "srv-exemple01"), ("level", "error"), ("type", "app")];
        // Dans l'ordre du temps, comme Render les rend : la plus récente est la dernière.
        let v = journal(
            vec![
                ligne("2026-10-26T15:00:00.000000001Z", "ERROR: une vieille erreur", &erreur),
                ligne("2026-10-27T11:50:12.5Z", "ERROR: relation \"factures\" absente", &erreur),
                ligne("2026-10-27T12:30:00.123456789Z", "  ERROR:  la connexion est refusée\n  détail", &erreur),
                ligne("pas une heure", "ERROR: sans heure", &erreur),
            ],
            false,
        );
        let e = erreurs_de(&v, &api(), MAINTENANT).unwrap();
        assert_eq!(
            (e.nom.as_str(), e.sorte, e.nombre, e.plus, e.derniere_heure),
            ("api-exemple", Sorte::Web, 4, false, 2)
        );
        let derniere = e.derniere.unwrap();
        assert_eq!(
            derniere,
            Ligne { quand: MAINTENANT - 4 * 60 - 32, message: "ERROR:  la connexion est refusée".into() }
        );
        // Rien : aucune erreur ; davantage que la lecture : `plus`.
        let vide = erreurs_de(&journal(vec![], false), &api(), MAINTENANT).unwrap();
        assert_eq!((vide.nombre, vide.derniere_heure, vide.derniere), (0, 0, None));
        let cent: Vec<Value> = (0..100).map(|_| ligne("2026-10-27T12:00:00Z", "ERROR: x", &erreur)).collect();
        let e = erreurs_de(&journal(cent, true), &api(), MAINTENANT).unwrap();
        assert_eq!((e.nombre, e.plus, e.derniere_heure), (100, true, 100));
        assert!(erreurs_de(&json!({"logs": "rien"}), &api(), MAINTENANT).is_err());
    }

    #[test]
    fn une_ligne_de_requete_se_dit_par_ses_etiquettes() {
        let etiquettes = [
            ("resource", "srv-exemple01"),
            ("method", "POST"),
            ("statusCode", "500"),
            ("host", "app-exemple.onrender.com"),
            ("path", "/api/factures"),
            ("level", "error"),
            ("type", "request"),
        ];
        let message = "clientIP=\"192.0.2.10\" requestID=\"abc\" responseTimeMS=12 userAgent=\"Exemple/1.0\"";
        let m = message_de(&ligne("2026-10-27T12:00:00Z", message, &etiquettes));
        assert_eq!(m, "HTTP 500 · POST /api/factures");
        assert!(!m.contains("192.0.2.10"));
        // Sans ses étiquettes, son texte.
        assert_eq!(message_de(&ligne("2026-10-27T12:00:00Z", "texte", &[("type", "request")])), "texte");
    }

    #[test]
    fn une_ligne_se_nettoie() {
        let m = |t: &str| message_de(&ligne("2026-10-27T12:00:00Z", t, &[("type", "app")]));
        assert_eq!(m("\u{1b}[31;1mERROR\u{1b}[0m:\tla base\u{7} répond mal"), "ERROR: la base répond mal");
        assert_eq!(m("\n\n   \r\n  Traceback (most recent call last):\n  File"), "Traceback (most recent call last):");
        assert_eq!(m(""), "");
        let longue = "é".repeat(400);
        let coupee = m(&longue);
        assert_eq!(coupee.chars().count(), MESSAGE_MAX);
        assert!(coupee.ends_with('…'));
        // Une clé ou un jeton cités ne s'affichent pas.
        let m2 = m("échec avec la clé sk_live_51Exemple pour le client");
        assert_eq!(m2, "échec avec la clé … pour le client");
        assert_eq!(m("Authorization: Bearer abc.def.ghi refusé"), "Authorization: Bearer …");
        assert_eq!(m("cle=rnd_ExempleInvente42 rejetée"), "… rejetée");
    }

    #[test]
    fn une_ligne_de_postgres_se_montre_a_partir_de_sa_gravite() {
        let m = |t: &str| message_de(&ligne("2026-10-27T12:00:00Z", t, &[("type", "app")]));
        let prefixe = "[6adf0000.f00-5] user=postgres,db=base_exemple,app=[unknown],client=10.0.0.12,";
        assert_eq!(
            m(&format!("{prefixe}ERROR:  canceling statement due to statement timeout")),
            "ERROR:  canceling statement due to statement timeout"
        );
        assert_eq!(
            m(&format!("{prefixe}FATAL:  password authentication failed")),
            "FATAL:  password authentication failed"
        );
        // Sans ce préfixe, rien ne se coupe.
        assert_eq!(m("échec : ERROR: la base répond mal"), "échec : ERROR: la base répond mal");
        assert_eq!(m("[worker] user=x sans gravité"), "[worker] user=x sans gravité");
    }

    /// Un faux Render : il rend les listes et les journaux inventés, et note chaque requête (sa route, ses
    /// paramètres).
    struct FauxRender {
        demandes: Vec<(String, Vec<(String, String)>)>,
        reponse: Box<dyn FnMut(&str, &[(&str, String)]) -> Result<Value, Manque>>,
    }

    /// Les lignes de requête d'un service web dont le code porte un 5, comme les rend le filtre de Render : une 500
    /// au niveau « warning », une 502 au niveau « error » (déjà parmi les erreurs), une 405 (un 5, mais pas une 5xx),
    /// une 503 au niveau « info », il y a trois heures.
    fn requetes_5xx() -> Value {
        let requete = |code: &'static str, niveau: &'static str, quoi: &'static str| {
            let (methode, chemin) = quoi.split_once(' ').unwrap();
            [("method", methode), ("statusCode", code), ("path", chemin), ("level", niveau), ("type", "request")]
        };
        let texte = "clientIP=\"192.0.2.10\" requestID=\"abc\" responseTimeMS=12";
        journal(
            vec![
                ligne("2026-10-27T09:30:00Z", texte, &requete("503", "info", "GET /")),
                ligne("2026-10-27T11:50:00Z", texte, &requete("405", "warning", "PUT /api")),
                ligne("2026-10-27T11:54:32Z", texte, &requete("500", "warning", "POST /api/factures")),
                ligne("2026-10-27T12:00:00Z", texte, &requete("502", "error", "GET /api/projets")),
            ],
            false,
        )
    }

    impl FauxRender {
        fn ordinaire() -> FauxRender {
            FauxRender {
                demandes: Vec::new(),
                reponse: Box::new(|route, parametres| {
                    let erreur = [("level", "error"), ("type", "app")];
                    let valeur = |nom: &str| parametres.iter().find(|(n, _)| *n == nom).map(|(_, v)| v.as_str());
                    Ok(match (route, valeur("resource"), valeur("type")) {
                        ("/v1/services", _, _) => services(),
                        ("/v1/postgres", _, _) => bases(),
                        (_, Some("srv-exemple02"), Some("request")) => requetes_5xx(),
                        (_, Some("dpg-exemple01"), _) => journal(
                            vec![
                                ligne("2026-10-27T03:00:00Z", "ERROR: première", &erreur),
                                ligne("2026-10-27T12:10:00Z", "ERROR: dernière", &erreur),
                            ],
                            false,
                        ),
                        _ => journal(vec![], false),
                    })
                }),
            }
        }

        fn obtenir(&mut self, route: &str, parametres: &[(&str, String)]) -> Result<Value, Manque> {
            let notes = parametres.iter().map(|(n, v)| ((*n).to_string(), v.clone())).collect();
            self.demandes.push((route.to_string(), notes));
            (self.reponse)(route, parametres)
        }
    }

    /// Des pages qui répondent toutes, comme `get_public` : la page en panne pour GitHub, les autres en ordre.
    fn pages(url: &str) -> Result<(u16, Vec<u8>), String> {
        let v = if url.contains("githubstatus") { page_en_panne() } else { page_en_ordre() };
        Ok((200, octets(&v)))
    }

    #[test]
    fn une_lecture_entiere() {
        let mut faux = FauxRender::ordinaire();
        let mut memoire: Memoire = vec![None; PAGES.len()];
        let l = lire(MAINTENANT, &mut memoire, &mut pages, &mut |r, p| faux.obtenir(r, p));
        let r = &l.releve;
        assert_eq!(r.lu_a, MAINTENANT);
        let noms: Vec<(&str, &str)> = r.statuts.iter().map(|s| (s.nom.as_str(), s.page.as_str())).collect();
        assert_eq!(
            noms,
            [
                ("Claude", "https://status.claude.com"),
                ("GitHub", "https://www.githubstatus.com"),
                ("Render", "https://status.render.com")
            ]
        );
        let niveaux: Vec<Niveau> = r.statuts.iter().map(|s| s.etat.as_ref().unwrap().niveau).collect();
        assert_eq!(niveaux, [Niveau::Operationnel, Niveau::Majeur, Niveau::Operationnel]);
        // Les erreurs de chaque ressource, dans l'ordre : les services web, la base, l'autre service.
        let erreurs = r.render.as_ref().unwrap();
        let vues: Vec<(&str, usize, usize)> =
            erreurs.iter().map(|e| (e.nom.as_str(), e.nombre, e.derniere_heure)).collect();
        assert_eq!(
            vues,
            [("api-exemple", 0, 0), ("app-exemple", 0, 0), ("Base_Exemple", 2, 1), ("travailleur-exemple", 0, 0)]
        );
        assert_eq!(erreurs[2].derniere.as_ref().unwrap().message, "ERROR: dernière");
        // Les réponses 5xx d'un service web : la 500 et la 503 ; ni la 502, au niveau « error », ni la 405.
        let attendues = Reponses5xx {
            nombre: 2,
            plus: false,
            derniere_heure: 1,
            derniere: Some(Ligne { quand: MAINTENANT - 40 * 60, message: "HTTP 500 · POST /api/factures".into() }),
        };
        assert_eq!(erreurs[1].reponses_5xx, attendues);
        assert!(erreurs.iter().filter(|e| e.nom != "app-exemple").all(|e| e.reponses_5xx == Reponses5xx::default()));
        // Les requêtes : trois pages, deux listes, un journal par ressource, et les requêtes de chaque service web —
        // des routes de la liste seulement.
        assert_eq!(l.requetes, 3 + 2 + 4 + 2);
        assert!(faux.demandes.iter().all(|(route, _)| ROUTES_LUES.contains(&route.as_str())));
        let routes: Vec<&str> = faux.demandes.iter().map(|(r, _)| r.as_str()).collect();
        assert_eq!(routes, ["/v1/services", "/v1/postgres"].into_iter().chain(["/v1/logs"; 6]).collect::<Vec<_>>());
        // La lecture d'un journal : l'équipe, la ressource, le niveau, les 24 h, les plus récentes d'abord, cent.
        let notes = |i: usize| faux.demandes[i].1.iter().map(|(n, v)| (n.as_str(), v.as_str())).collect::<Vec<_>>();
        let (debut, fin) = (("startTime", "2026-10-26T12:34:32Z"), ("endTime", "2026-10-27T12:34:32Z"));
        let (sens, cent) = (("direction", "backward"), ("limit", "100"));
        let equipe = ("ownerId", "tea-exemple01");
        let erreurs_de_la_base = [equipe, ("resource", "dpg-exemple01"), ("level", "error"), debut, fin, sens, cent];
        assert_eq!(notes(6), erreurs_de_la_base);
        // Celle des requêtes d'un service web : ses lignes de requête dont le code porte un 5.
        let requetes = [equipe, ("resource", "srv-exemple02"), ("type", "request"), ("statusCode", "5*")];
        assert_eq!(notes(5), [&requetes[..], &[debut, fin, sens, cent]].concat());
        assert_eq!(notes(0), [("limit", "100")]);
        assert!(l.manquees.iter().all(Option::is_none) && !l.cle_refusee);
        // Le résumé du journal : des nombres et des états, aucun nom de ressource ni ligne.
        let resume = r.resume();
        assert_eq!(
            resume,
            "Claude opérationnel, GitHub en panne partielle, Render opérationnel ; sur Render, 2 services web, 1 \
             base et 1 autre service : 2 erreurs en 24 h, dont 1 dans la dernière heure ; 2 réponses HTTP 5xx hors \
             des erreurs, dont 1 dans la dernière heure"
        );
        assert!(!resume.contains("exemple") && !resume.contains("ERROR") && !resume.contains("/api"));
    }

    #[test]
    fn les_reponses_5xx_d_un_service_web() {
        let requete = |code: &'static str, niveau: Option<&'static str>| {
            let mut e = vec![("type", "request"), ("statusCode", code), ("method", "GET"), ("path", "/")];
            e.extend(niveau.map(|n| ("level", n)));
            ligne("2026-10-27T12:30:00Z", "texte", &e)
        };
        let v = journal(
            vec![
                requete("500", Some("warning")),
                requete("599", Some("info")),
                requete("504", None),
                requete("503", Some("error")),
                requete("450", Some("warning")),
                requete("5", Some("warning")),
                requete("1500", Some("warning")),
                requete("50x", Some("warning")),
            ],
            true,
        );
        let r = reponses_5xx_de(&v, MAINTENANT).unwrap();
        assert_eq!((r.nombre, r.plus, r.derniere_heure), (3, true, 3), "500, 599 et 504 ; davantage à lire");
        assert_eq!(reponses_5xx_de(&journal(vec![], false), MAINTENANT).unwrap(), Reponses5xx::default());
        assert!(reponses_5xx_de(&json!({}), MAINTENANT).is_err());
    }

    #[test]
    fn une_cle_refusee_se_dit() {
        let mut memoire: Memoire = vec![None; PAGES.len()];
        let mut refus = |_: &str, _: &[(&str, String)]| reponse_de_render(401, b"{\"message\":\"unauthorized\"}");
        let l = lire(MAINTENANT, &mut memoire, &mut pages, &mut refus);
        assert!(l.cle_refusee);
        let e = l.releve.render.unwrap_err();
        assert!(e.contains("401") && e.contains("eve --cle render"), "{e}");
        assert_eq!(l.requetes, 3 + 1, "rien d'autre ne part après le refus");
        // Les pages, elles, se lisent.
        assert!(l.releve.statuts.iter().all(|s| s.etat.is_ok()));
    }

    #[test]
    fn les_reponses_de_render() {
        assert!(matches!(reponse_de_render(200, b"[]"), Ok(Value::Array(_))));
        assert_eq!(
            reponse_de_render(200, b"<html>"),
            Err(Manque::Panne("la réponse de Render n'est pas du JSON".into()))
        );
        assert!(matches!(reponse_de_render(401, b""), Err(Manque::Cle(_))));
        assert!(matches!(reponse_de_render(403, b""), Err(Manque::Cle(t)) if t.contains("403")));
        assert_eq!(reponse_de_render(429, b""), Err(Manque::Panne("Render demande d'attendre (HTTP 429)".into())));
        assert_eq!(reponse_de_render(503, b""), Err(Manque::Panne("l'API de Render ne répond pas (HTTP 503)".into())));
        let refus = br#"{"id": "invalid", "message": "invalid resource\r\nid"}"#;
        assert_eq!(
            reponse_de_render(400, refus),
            Err(Manque::Panne("Render répond HTTP 400 : invalid resource".into()))
        );
        assert_eq!(reponse_de_render(404, b""), Err(Manque::Panne("Render répond HTTP 404".into())));
    }

    #[test]
    fn une_requete_ne_vise_que_les_routes_lues() {
        let p = [("ownerId", "tea-exemple01".to_string()), ("startTime", "2026-10-26T12:34:32Z".to_string())];
        assert_eq!(
            adresse_de_render("/v1/logs", &p).unwrap(),
            "https://api.render.com/v1/logs?ownerId=tea-exemple01&startTime=2026-10-26T12%3A34%3A32Z"
        );
        assert_eq!(adresse_de_render("/v1/services", &[]).unwrap(), "https://api.render.com/v1/services");
        // Toute autre route est refusée avant l'envoi : celle qui suspendrait un service, une adresse glissée.
        for route in ["/v1/services/srv-exemple01/suspend", "/v1/services/srv-exemple01", "/v1/logs/../owners", "//x"] {
            let e = adresse_de_render(route, &[]).unwrap_err();
            assert!(e.contains("rien n'est envoyé"), "{route}");
            assert!(obtenir_de_render("rnd_EssaiInvente42", route, &[]).is_err(), "{route}");
        }
        // Un paramètre ne glisse rien de plus dans l'adresse.
        let glisse = [("resource", "srv-x&level=info".to_string())];
        assert!(adresse_de_render("/v1/logs", &glisse).unwrap().ends_with("resource=srv-x%26level%3Dinfo"));
    }

    #[test]
    fn une_page_manquee_garde_sa_lecture_une_heure() {
        let mut faux = FauxRender::ordinaire();
        let mut memoire: Memoire = vec![None; PAGES.len()];
        let _ = lire(MAINTENANT, &mut memoire, &mut pages, &mut |r, p| faux.obtenir(r, p));
        // GitHub ne répond plus : sa lecture d'avant reste une heure, puis sa panne se montre.
        let mut sans_github = |url: &str| {
            if url.contains("githubstatus") {
                Err("le serveur ne répond pas à temps".to_string())
            } else {
                pages(url)
            }
        };
        let l = lire(MAINTENANT + 3600, &mut memoire, &mut sans_github, &mut |r, p| faux.obtenir(r, p));
        assert_eq!(l.releve.statuts[1].etat.as_ref().unwrap().niveau, Niveau::Majeur);
        assert_eq!(l.manquees[1].as_deref(), Some("la page d'état ne répond pas : le serveur ne répond pas à temps"));
        let l = lire(MAINTENANT + 3601, &mut memoire, &mut sans_github, &mut |r, p| faux.obtenir(r, p));
        assert!(l.releve.statuts[1].etat.as_ref().unwrap_err().contains("ne répond pas"));
        assert!(l.releve.statuts[0].etat.is_ok() && l.releve.statuts[2].etat.is_ok());
    }

    #[test]
    fn une_requete_de_render_qui_manque_fait_manquer_la_lecture() {
        let mut memoire: Memoire = vec![None; PAGES.len()];
        let mut ordinaire = FauxRender::ordinaire();
        let mut coupe = |route: &str, p: &[(&str, String)]| {
            if p.iter().any(|(_, v)| v == "dpg-exemple01") {
                Err(Manque::Panne("l'API de Render ne répond pas : la connexion a été coupée".into()))
            } else {
                ordinaire.obtenir(route, p)
            }
        };
        let l = lire(MAINTENANT, &mut memoire, &mut pages, &mut coupe);
        assert_eq!(l.releve.render, Err("l'API de Render ne répond pas : la connexion a été coupée".into()));
        assert!(!l.cle_refusee);
    }

    /// Une lecture inventée, pour le suivi : ses pages à ces niveaux (`None` : manquée), Render en ordre avec tant
    /// d'erreurs dans la dernière heure, ou en panne.
    fn lecture(niveaux: [Option<Niveau>; 3], render: Result<usize, Manque>) -> Lecture {
        let statuts = PAGES
            .iter()
            .zip(niveaux)
            .map(|((nom, page, _), n)| Statut {
                nom: (*nom).to_string(),
                page: (*page).to_string(),
                etat: n
                    .map(|niveau| Etat { niveau, incidents: vec![], composants: vec![] })
                    .ok_or_else(|| "rien".into()),
            })
            .collect();
        let manquees =
            niveaux.iter().map(|n| n.is_none().then(|| "le serveur ne répond pas à temps".to_string())).collect();
        let cle_refusee = matches!(render, Err(Manque::Cle(_)));
        let render = render.map(|recentes| {
            vec![Erreurs {
                nom: "api-exemple".into(),
                sorte: Sorte::Web,
                nombre: recentes + 3,
                plus: false,
                derniere_heure: recentes,
                derniere: None,
                reponses_5xx: Reponses5xx::default(),
            }]
        });
        Lecture {
            releve: Releve { lu_a: MAINTENANT, statuts, render: render.map_err(|m| m.texte().to_string()) },
            manquees,
            cle_refusee,
            requetes: 6,
            duree: Duration::from_millis(1400),
        }
    }

    /// La même lecture, avec tant de réponses 5xx dans la dernière heure.
    fn avec_5xx(mut l: Lecture, recentes: usize) -> Lecture {
        if let Ok(liste) = &mut l.releve.render {
            liste[0].reponses_5xx = Reponses5xx { nombre: recentes, derniere_heure: recentes, ..Default::default() };
        }
        l
    }

    const BIEN: [Option<Niveau>; 3] = [Some(Niveau::Operationnel); 3];

    #[test]
    fn une_panne_se_dit_une_fois() {
        let t0 = instant();
        let mut s = Suivi::nouveau(t0);
        assert!(s.a_lire(t0));
        // La première lecture se dit ; la suivante, dans 5 min.
        assert_eq!(
            s.apres(&lecture(BIEN, Ok(0)), t0),
            ["Services : Claude opérationnel, GitHub opérationnel, Render opérationnel ; sur Render, 1 service web : \
                 3 erreurs en 24 h, dont 0 dans la dernière heure (lus en 1,4 s, 6 requêtes ; relus toutes les 5 \
                 min)."]
        );
        assert!(!s.a_lire(t0 + Duration::from_secs(4 * 60)));
        assert!(s.a_lire(t0 + PERIODE));
        // Render en panne : une ligne, puis rien jusqu'au retour ; les essais reprennent vite, puis s'espacent.
        let panne = Manque::Panne("l'API de Render ne répond pas (HTTP 503)".into());
        let mut t = t0 + PERIODE;
        let (mut dites, mut attentes) = (Vec::new(), Vec::new());
        for _ in 0..6 {
            dites.extend(s.apres(&lecture(BIEN, Err(panne.clone())), t));
            let attente = s.prochaine.duration_since(t).unwrap();
            attentes.push(attente.as_secs() / 60);
            t += attente;
            assert!(s.a_lire(t));
        }
        let dite = "Services : les erreurs de Render ne se lisent pas : l'API de Render ne répond pas (HTTP 503) ; \
                    nouvel essai dans 1 min (une seule ligne jusqu'au retour).";
        assert_eq!(dites, [dite]);
        assert_eq!(attentes, [1, 2, 4, 8, 8, 8]);
        // Le retour se dit, une fois ; la période reprend, et la panne suivante repart à une minute.
        assert_eq!(s.apres(&lecture(BIEN, Ok(0)), t), ["Services : les erreurs de Render se lisent de nouveau."]);
        assert_eq!(s.prochaine.duration_since(t).unwrap(), PERIODE);
        assert_eq!(s.apres(&lecture(BIEN, Err(panne)), t).len(), 1);
        assert_eq!(s.prochaine.duration_since(t).unwrap(), REPRISE);
    }

    #[test]
    fn une_cle_refusee_ne_presse_pas_les_essais() {
        let t = instant();
        let mut s = Suivi::nouveau(t);
        let _ = s.apres(&lecture(BIEN, Ok(0)), t);
        let refus = Manque::Cle(CLE_REFUSEE.into());
        let dites = s.apres(&lecture(BIEN, Err(refus.clone())), t);
        assert_eq!(dites.len(), 1);
        assert!(dites[0].contains("eve --cle render") && dites[0].contains("dès que la clé change"), "{}", dites[0]);
        assert_eq!(s.prochaine.duration_since(t).unwrap(), PERIODE);
        assert!(s.apres(&lecture(BIEN, Err(refus)), t).is_empty());
        // Le réseau coupé : aucune page ne répond, Render non plus ; les essais reprennent vite.
        let coupe = Manque::Panne("l'API de Render ne répond pas : le nom du serveur est introuvable".into());
        let _ = s.apres(&lecture([None; 3], Err(coupe)), t);
        assert_eq!(s.prochaine.duration_since(t).unwrap(), REPRISE);
    }

    #[test]
    fn une_page_qui_change_se_dit() {
        let t = instant();
        let mut s = Suivi::nouveau(t);
        let _ = s.apres(&lecture(BIEN, Ok(0)), t);
        let github = [Some(Niveau::Operationnel), Some(Niveau::Majeur), Some(Niveau::Operationnel)];
        assert_eq!(s.apres(&lecture(github, Ok(0)), t), ["Services : GitHub — en panne partielle."]);
        assert!(s.apres(&lecture(github, Ok(0)), t).is_empty());
        assert_eq!(s.apres(&lecture(BIEN, Ok(0)), t), ["Services : GitHub — opérationnel de nouveau."]);
        // Une page qui ne répond plus : une ligne, puis rien, puis son retour ; une seule page ne presse pas les
        // essais.
        let sans_claude = [None, Some(Niveau::Operationnel), Some(Niveau::Operationnel)];
        let dites = s.apres(&lecture(sans_claude, Ok(0)), t);
        assert_eq!(
            dites,
            ["Services : la page d'état de Claude ne se lit pas (le serveur ne répond pas à temps) ; sa dernière \
                 lecture reste une heure."]
        );
        assert_eq!(s.prochaine.duration_since(t).unwrap(), PERIODE);
        assert!(s.apres(&lecture(sans_claude, Ok(0)), t).is_empty());
        assert_eq!(s.apres(&lecture(BIEN, Ok(0)), t), ["Services : la page d'état de Claude se lit de nouveau."]);
    }

    #[test]
    fn des_erreurs_dans_la_derniere_heure_se_disent() {
        let t = instant();
        let mut s = Suivi::nouveau(t);
        let _ = s.apres(&lecture(BIEN, Ok(0)), t);
        assert_eq!(
            s.apres(&lecture(BIEN, Ok(4)), t),
            ["Services : sur Render, 4 erreurs dans la dernière heure (1 ressource)."]
        );
        assert!(s.apres(&lecture(BIEN, Ok(6)), t).is_empty());
        assert_eq!(s.apres(&lecture(BIEN, Ok(0)), t), ["Services : sur Render, plus d'erreur dans la dernière heure."]);
        // Des réponses 5xx comptent aussi pour la dernière heure, seules ou avec des erreurs.
        assert_eq!(
            s.apres(&avec_5xx(lecture(BIEN, Ok(0)), 1), t),
            ["Services : sur Render, 1 réponse HTTP 5xx dans la dernière heure (1 ressource)."]
        );
        assert!(s.apres(&avec_5xx(lecture(BIEN, Ok(3)), 2), t).is_empty());
        let _ = s.apres(&lecture(BIEN, Ok(0)), t);
        assert_eq!(
            s.apres(&avec_5xx(lecture(BIEN, Ok(3)), 2), t),
            ["Services : sur Render, 3 erreurs et 2 réponses HTTP 5xx dans la dernière heure (1 ressource)."]
        );
        // Des erreurs dès la première lecture : son résumé le dit, rien de plus.
        let mut s = Suivi::nouveau(t);
        assert_eq!(s.apres(&lecture(BIEN, Ok(2)), t).len(), 1);
        assert!(s.apres(&lecture(BIEN, Ok(2)), t).is_empty());
    }

    #[test]
    fn l_heure_de_lire() {
        let t = instant();
        let mut s = Suivi::nouveau(t);
        s.prochaine = t + REPRISE_MAX;
        assert!(!s.a_lire(t), "une reprise de 8 min n'est pas une horloge qui recule");
        assert!(s.a_lire(t + REPRISE_MAX));
        // L'horloge a reculé d'une heure : la lecture prévue serait dans 1 h 08 ; elle se fait tout de suite.
        assert!(s.a_lire(t - Duration::from_secs(3600)));
    }

    #[test]
    fn une_lecture_par_minute_au_plus() {
        let t = Instant::now();
        assert_eq!(reste_avant(None, t), None);
        assert_eq!(reste_avant(Some(t), t), Some(ENTRE_DEUX_LECTURES));
        assert_eq!(reste_avant(Some(t), t + Duration::from_secs(45)), Some(Duration::from_secs(15)));
        assert_eq!(reste_avant(Some(t), t + ENTRE_DEUX_LECTURES), None);
    }

    #[test]
    fn les_mots_du_resume() {
        assert_eq!(nombre(0, "erreur", "erreurs"), "0 erreur");
        assert_eq!(nombre(2, "erreur", "erreurs"), "2 erreurs");
        assert_eq!(et(&[]), "");
        assert_eq!(et(&["a".into()]), "a");
        assert_eq!(et(&["a".into(), "b".into(), "c".into()]), "a, b et c");
        assert_eq!(resume_de_render(&[]), "sur Render, aucune ressource");
        let e = Erreurs {
            nom: "base-exemple".into(),
            sorte: Sorte::Base,
            nombre: 100,
            plus: true,
            derniere_heure: 0,
            derniere: None,
            reponses_5xx: Reponses5xx::default(),
        };
        assert_eq!(
            resume_de_render(std::slice::from_ref(&e)),
            "sur Render, 1 base : au moins 100 erreurs en 24 h, dont 0 dans la dernière heure"
        );
        let web = Erreurs {
            sorte: Sorte::Web,
            nombre: 1,
            plus: false,
            reponses_5xx: Reponses5xx { nombre: 100, plus: true, derniere_heure: 7, derniere: None },
            ..e
        };
        assert_eq!(
            resume_de_render(&[web]),
            "sur Render, 1 service web : 1 erreur en 24 h, dont 0 dans la dernière heure ; au moins 100 réponses \
             HTTP 5xx hors des erreurs, dont 7 dans la dernière heure"
        );
        assert_eq!(erreurs_en_mots(0, 0), "0 erreur");
        assert_eq!(erreurs_en_mots(0, 1), "1 réponse HTTP 5xx");
        assert_eq!(erreurs_en_mots(2, 3), "2 erreurs et 3 réponses HTTP 5xx");
    }
}
