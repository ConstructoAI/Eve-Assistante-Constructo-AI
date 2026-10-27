//! Les montants de Stripe, pour le tableau de bord (le thème « Constructo AI ») : le solde du compte, ce qui est entré
//! aujourd'hui et ce mois-ci, le revenu mensuel des abonnements, le prochain virement, les factures qui attendent leur
//! paiement, les derniers paiements — lus par l'API de Stripe (`https://api.stripe.com/v1`, en GET seulement), avec la
//! clé d'API que garde le dossier local (`cles.rs` : une clé restreinte, en lecture seule, suffit). Seulement chez qui
//! en a posé une (`eve --cle stripe`) : sans elle, rien ne se lit, et le tableau n'en montre rien.
//!
//! C'est le compte d'une entreprise, et Eve n'y fait que lire : les routes nommées une à une ([`Route`]), par une seule
//! fonction dont le verbe est écrit en dur ([`get`]) — jamais un POST, qui chez Stripe crée un objet réel (un client,
//! un paiement). La clé part dans l'en-tête `Authorization`, jamais dans une adresse ; la version de l'API est fixée
//! ([`VERSION`]) : les réponses gardent leur forme quand Stripe en publie une autre.
//!
//! Le fil : une lecture tout de suite, puis toutes les 10 min ; après une panne, dans 1 min, puis 2, 4, 8. Il regarde
//! le fichier de la clé toutes les 30 s : une clé posée se lit aussitôt, une clé retirée ne montre plus rien. Une
//! lecture manquée en passant (le réseau, Stripe qui flanche) laisse les derniers montants une heure ; une clé refusée
//! (401), ou à qui il manque un droit (403, une clé restreinte), se dit tout de suite à leur place, avec les droits à
//! lui donner. Au journal, une ligne par lecture : sa durée et ses requêtes ; jamais un montant, un client ni la clé.

use std::fmt::Write as _;
use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::Value;

use crate::cles::{self, Service};
use crate::{centre, entreprise, https, journal};

/// Ce qu'une lecture a compté : un montant (en dollars, l'unité de la devise), un nombre de paiements, et si la
/// lecture s'est arrêtée avant la fin (`plus` : le nombre et le montant sont alors au moins ceux-là).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Somme {
    pub montant: f64,
    pub nombre: usize,
    pub plus: bool,
}

/// Les abonnements actifs : combien, et ce qu'ils rapportent par mois (un abonnement annuel compte pour son douzième).
#[derive(Clone, Debug, PartialEq)]
pub struct Abonnements {
    pub actifs: usize,
    pub mensuel: f64,
}

/// Un virement de Stripe vers le compte en banque : son montant, le jour où il arrive (« 2026-10-29 »), s'il est fait.
#[derive(Clone, Debug, PartialEq)]
pub struct Virement {
    pub montant: f64,
    pub arrivee: String,
    pub fait: bool,
}

/// Les factures ouvertes (envoyées, pas encore payées) : combien (`plus` : davantage que la lecture n'en a compté),
/// leur total dû, combien ont passé leur échéance — pour une facture payée d'office (sans échéance), dès qu'un paiement
/// a été tenté sans la régler.
#[derive(Clone, Debug, PartialEq)]
pub struct Ouvertes {
    pub nombre: usize,
    pub plus: bool,
    pub du: f64,
    pub echues: usize,
}

/// L'issue d'un paiement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Issue {
    Reussi,
    Echoue,
    Rembourse,
    EnAttente,
}

/// Un paiement : son montant (dans sa devise : celle du compte, d'ordinaire), son heure (en secondes Unix), qui a payé
/// (le nom du client, sinon son courriel, sinon la description du paiement), son issue.
#[derive(Clone, Debug, PartialEq)]
pub struct Paiement {
    pub montant: f64,
    pub quand: i64,
    pub qui: String,
    pub issue: Issue,
}

/// Les montants du compte Stripe à leur lecture (en secondes Unix) : sa devise (« CAD »), son solde disponible et en
/// attente, ce qui est entré aujourd'hui et ce mois-ci (les paiements réussis, remboursements ôtés, avant les frais de
/// Stripe), les abonnements, le prochain virement (sinon le dernier fait), les factures ouvertes, les derniers
/// paiements (cinq au plus, le plus récent d'abord). Ce qu'une lecture n'a pas pu lire manque (`None`).
#[derive(Clone, Debug, PartialEq)]
pub struct Montants {
    pub lu_a: i64,
    pub devise: String,
    pub disponible: f64,
    pub en_attente: f64,
    pub aujourdhui: Somme,
    pub ce_mois: Somme,
    pub abonnements: Option<Abonnements>,
    pub virement: Option<Virement>,
    pub ouvertes: Option<Ouvertes>,
    pub paiements: Vec<Paiement>,
}

/// Des montants imposés : la vitrine de `eve --essai-theme` (un compte inventé). Tant qu'ils le sont, tout suivi les
/// rend tels quels, sans fil ni requête. Eve ne les impose jamais.
static IMPOSE: Mutex<Option<Result<Montants, String>>> = Mutex::new(None);

/// Impose ces montants (ou cette panne) à tous les suivis, ceux qui tournent comme ceux qui viendront.
pub fn imposer(m: Result<Montants, String>) {
    *IMPOSE.lock().unwrap_or_else(PoisonError::into_inner) = Some(m);
}

fn impose() -> Option<Result<Montants, String>> {
    IMPOSE.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

/// Le suivi de Stripe, que son fil tient à jour. Lâché, son fil s'arrête.
pub struct Stripe {
    partage: Arc<Mutex<Partage>>,
    /// Lâché avec le suivi : son fil le voit et s'arrête.
    _arret: mpsc::Sender<()>,
}

impl Stripe {
    /// Lance le suivi, dans son fil (aucun sous des montants imposés : [`imposer`]).
    pub fn lancer() -> Stripe {
        let partage: Arc<Mutex<Partage>> = Arc::default();
        let (arret, attente) = mpsc::channel();
        let p = Arc::clone(&partage);
        if impose().is_none()
            && let Err(e) = thread::Builder::new().name("stripe".into()).spawn(move || suivre(&p, &attente))
        {
            journal::ecrire(&format!("Stripe : le fil ne démarre pas : {e}"));
        }
        Stripe { partage, _arret: arret }
    }

    /// Les derniers montants lus (d'une heure au plus), ou la raison de la panne quand il n'y en a pas ; rien sans clé
    /// de Stripe, ni avant la première lecture. Des montants imposés passent avant tout.
    pub fn montants(&self) -> Option<Result<Montants, String>> {
        impose().or_else(|| self.partage.lock().unwrap_or_else(PoisonError::into_inner).montants(SystemTime::now()))
    }
}

// ---- Les routes, et les droits qu'elles demandent ------------------------------------------------------------------

/// L'API de Stripe.
const API: &str = "https://api.stripe.com";
/// La version de l'API que demande Eve : celle du compte le 27 octobre 2026. Les réponses gardent la forme que lit ce
/// module, quelle que soit la version que Stripe donne ensuite au compte.
pub const VERSION: &str = "2025-11-17.clover";

/// Un droit qu'il faut à une clé restreinte, en lecture (« Read ») : son nom dans le tableau de bord de Stripe
/// (Développeurs › Clés API), et celui que Stripe donne en le refusant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Droit {
    pub nom: &'static str,
    pub code: &'static str,
}

const BALANCE: Droit = Droit { nom: "Balance", code: "rak_balance_read" };
const CHARGES: Droit = Droit { nom: "Charges", code: "rak_charge_read" };
const CUSTOMERS: Droit = Droit { nom: "Customers", code: "rak_customer_read" };
const SUBSCRIPTIONS: Droit = Droit { nom: "Subscriptions", code: "rak_subscription_read" };
const INVOICES: Droit = Droit { nom: "Invoices", code: "rak_invoice_read" };
const PAYOUTS: Droit = Droit { nom: "Payouts", code: "rak_payout_read" };

/// Tous les droits que demande une lecture du compte, chacun une fois : ceux d'une clé restreinte pour Eve.
pub const DROITS: [Droit; 6] = [BALANCE, CHARGES, CUSTOMERS, SUBSCRIPTIONS, INVOICES, PAYOUTS];

/// Les seules routes de Stripe que lit Eve, toujours en GET ([`get`]) : nommées une à une, comme les outils de l'ERP
/// (`entreprise::OUTILS_LUS`). Une route s'ajoute ici, jamais ailleurs : aucune autre adresse ne se fait.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    /// Le solde, disponible et en attente, et la devise du compte.
    Solde,
    /// Les transactions du solde depuis le début du mois : ce qui est entré.
    Transactions,
    /// Les derniers paiements, et le nom de leur client.
    Paiements,
    /// Les abonnements actifs.
    Abonnements,
    /// Les factures ouvertes.
    Factures,
    /// Les virements vers le compte en banque.
    Virements,
}

impl Route {
    /// Dans l'ordre d'une lecture : le solde d'abord, dont la devise sert aux autres.
    pub const TOUTES: [Route; 6] =
        [Route::Solde, Route::Transactions, Route::Paiements, Route::Abonnements, Route::Factures, Route::Virements];

    /// Son chemin dans l'API.
    pub fn chemin(self) -> &'static str {
        match self {
            Route::Solde => "/v1/balance",
            Route::Transactions => "/v1/balance_transactions",
            Route::Paiements => "/v1/charges",
            Route::Abonnements => "/v1/subscriptions",
            Route::Factures => "/v1/invoices",
            Route::Virements => "/v1/payouts",
        }
    }

    /// Les droits qu'elle demande à une clé restreinte (les paiements lisent aussi leur client).
    pub fn droits(self) -> &'static [Droit] {
        match self {
            Route::Solde | Route::Transactions => &[BALANCE],
            Route::Paiements => &[CHARGES, CUSTOMERS],
            Route::Abonnements => &[SUBSCRIPTIONS],
            Route::Factures => &[INVOICES],
            Route::Virements => &[PAYOUTS],
        }
    }
}

// ---- Le fil ---------------------------------------------------------------------------------------------------------

/// Une lecture toutes les 10 min, Stripe répondant.
const PERIODE: Duration = Duration::from_secs(10 * 60);
/// Après une panne, un nouvel essai dans 1 min, puis 2, 4, 8, et 10 au plus : le réseau qui revient au réveil du poste
/// se voit vite ; une longue panne n'envoie pas plus de requêtes qu'une lecture aux dix minutes.
const REPRISE: Duration = Duration::from_secs(60);
/// Des montants restent bons une heure après leur lecture, le temps de quelques lectures manquées ; au-delà, la panne
/// se dit à leur place.
const VALIDITE: Duration = Duration::from_secs(60 * 60);
/// Le fil regarde l'heure et la clé toutes les 30 s : une clé posée ou retirée se voit vite, et une attente de Windows
/// ne compte pas le temps où le poste dort.
const PAS: Duration = Duration::from_secs(30);
/// Le délai d'une requête.
const DELAI: Duration = Duration::from_secs(20);
/// Au plus une lecture par minute, tous suivis confondus : une fenêtre qui relancerait le suivi à chaque image ne
/// ferait pas pour autant sept requêtes à Stripe trente fois par seconde.
const ENTRE_DEUX_LECTURES: Duration = Duration::from_secs(60);
/// Des montants d'il y a plus d'une heure, sans panne dite : le poste dormait, la lecture suivante vient.
const PERIMES: &str = "le compte n'a pas été relu depuis plus d'une heure";
/// Stripe refuse la clé (401).
const CLE_REFUSEE: &str =
    "la clé d'API est refusée (invalide, expirée ou révoquée) : la reposer par « eve --cle stripe »";
/// La clé posée ne se déchiffre pas (DPAPI : une autre session de Windows, un autre poste) ou n'en a plus la forme.
const CLE_ILLISIBLE: &str = "la clé d'API posée ne se lit pas ici (posée sous une autre session de Windows ?) : la \
                             reposer par « eve --cle stripe »";

/// Ce que le fil laisse au tableau.
#[derive(Default)]
struct Partage {
    /// Une clé de Stripe est posée : sans elle, rien ne se montre.
    cle: bool,
    /// Les derniers montants lus, et l'heure de leur lecture.
    dernier: Option<(Montants, SystemTime)>,
    /// La raison de la panne en cours, depuis la dernière lecture réussie.
    panne: Option<String>,
}

impl Partage {
    /// Ce que montre le tableau à `maintenant` : rien sans clé, ni avant la première lecture ; les derniers montants
    /// tant qu'ils ont une heure au plus ; sinon, pourquoi il n'y en a pas.
    fn montants(&self, maintenant: SystemTime) -> Option<Result<Montants, String>> {
        if !self.cle {
            return None;
        }
        match (&self.dernier, &self.panne) {
            (Some((m, lu)), _) if frais(*lu, maintenant) => Some(Ok(m.clone())),
            (_, Some(panne)) => Some(Err(panne.clone())),
            (Some(_), None) => Some(Err(PERIMES.into())),
            (None, None) => None,
        }
    }
}

/// Des montants lus à `lu` sont-ils encore bons à `maintenant` ? Une heure ; une horloge qui a reculé les garde jusqu'à
/// la lecture suivante.
fn frais(lu: SystemTime, maintenant: SystemTime) -> bool {
    !matches!(maintenant.duration_since(lu), Ok(age) if age > VALIDITE)
}

/// Ce qui dit qu'une clé a changé : la date et la taille de son fichier ; rien, pas de clé.
type Empreinte = Option<(SystemTime, u64)>;

fn empreinte_de_la_cle() -> Empreinte {
    let m = std::fs::metadata(cles::chemin(Service::Stripe)).ok().filter(|m| m.is_file())?;
    Some((m.modified().unwrap_or(UNIX_EPOCH), m.len()))
}

/// La clé a-t-elle changé depuis le dernier regard (`avant` : son empreinte d'alors ; rien au premier regard) ? Rien si
/// elle est la même ; sinon, la ligne qui le dit au journal — aucune au premier regard, le démarrage d'Eve.
fn changement(avant: Option<Empreinte>, cle: Empreinte) -> Option<Option<&'static str>> {
    let avant = match avant {
        Some(a) if a == cle => return None,
        Some(a) => a,
        None => return Some(None),
    };
    Some(Some(match (avant.is_some(), cle.is_some()) {
        (false, _) => "Stripe : une clé est posée ; elle se lit tout de suite.",
        (true, false) => "Stripe : la clé est retirée ; plus rien ne se lit.",
        (true, true) => "Stripe : la clé a changé ; elle se lit tout de suite.",
    }))
}

/// Le fil du suivi : il regarde la clé toutes les 30 s, et lit le compte quand [`Suivi`] le dit ; il s'arrête quand le
/// suivi est lâché (l'émetteur d'`arret` avec lui).
fn suivre(partage: &Mutex<Partage>, arret: &mpsc::Receiver<()>) {
    let mut suivi = Suivi::nouveau(SystemTime::now());
    // L'empreinte de la clé au dernier regard ; rien avant le premier.
    let mut vue: Option<Empreinte> = None;
    loop {
        let maintenant = SystemTime::now();
        let cle = empreinte_de_la_cle();
        if let Some(ligne) = changement(vue, cle) {
            if let Some(ligne) = ligne {
                journal::ecrire(ligne);
            }
            // Ce qu'une autre clé a lu ne se montre plus : ce peut être un autre compte.
            *partage.lock().unwrap_or_else(PoisonError::into_inner) =
                Partage { cle: cle.is_some(), ..Partage::default() };
            suivi = Suivi::nouveau(maintenant);
            vue = Some(cle);
        }
        if cle.is_some() && suivi.a_lire(maintenant) {
            if let Err(reste) = reserver() {
                suivi.prochaine = maintenant + reste;
            } else {
                lire_et_partager(&mut suivi, partage);
            }
        }
        if !matches!(arret.recv_timeout(PAS), Err(mpsc::RecvTimeoutError::Timeout)) {
            return;
        }
    }
}

/// Une lecture du compte, avec la clé du dossier local : ce qu'en dit le journal, et ce qu'en garde le partage.
fn lire_et_partager(suivi: &mut Suivi, partage: &Mutex<Partage>) {
    let debut = Instant::now();
    let lu = match cles::lire(Service::Stripe) {
        Some(c) => lire_maintenant(&c),
        None => Err(Panne::Refus(CLE_ILLISIBLE.into())),
    };
    let fin = SystemTime::now();
    let issue = lu.as_ref().map(|(_, requetes)| *requetes).map_err(Panne::texte);
    for ligne in suivi.apres(issue, debut.elapsed(), fin) {
        journal::ecrire(&ligne);
    }
    let mut p = partage.lock().unwrap_or_else(PoisonError::into_inner);
    match lu {
        Ok((m, _)) => {
            p.dernier = Some((m, fin));
            p.panne = None;
        }
        // La clé refusée, ou un droit qui lui manque : la raison se montre tout de suite, sans les montants.
        Err(Panne::Refus(e)) => {
            p.dernier = None;
            p.panne = Some(e);
        }
        Err(Panne::Passagere(e)) => p.panne = Some(e),
    }
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

/// Le fil entre deux lectures : quand lire la suivante, et ce qui se dit au journal.
struct Suivi {
    prochaine: SystemTime,
    /// L'attente après une panne, qui double à chaque échec, jusqu'à la période.
    reprise: Duration,
    en_panne: bool,
    deja_lu: bool,
}

impl Suivi {
    fn nouveau(maintenant: SystemTime) -> Suivi {
        Suivi { prochaine: maintenant, reprise: REPRISE, en_panne: false, deja_lu: false }
    }

    /// L'heure de lire est-elle venue ? Aussi quand l'horloge du poste a reculé de plus d'une période (une heure
    /// changée à la main) : la lecture suivante ne s'éloigne pas d'autant.
    fn a_lire(&self, maintenant: SystemTime) -> bool {
        !matches!(self.prochaine.duration_since(maintenant), Ok(reste) if !reste.is_zero() && reste <= PERIODE)
    }

    /// Après une lecture finie à `maintenant` (`issue` : ses requêtes, ou sa panne ; `duree` : son temps) : quand lire
    /// la suivante, et les lignes du journal — chaque lecture, sa durée et ses requêtes ; une panne, une seule ligne
    /// jusqu'au retour ; le retour.
    fn apres(&mut self, issue: Result<usize, &str>, duree: Duration, maintenant: SystemTime) -> Vec<String> {
        let mut lignes = Vec::new();
        match issue {
            Ok(requetes) => {
                if self.en_panne {
                    lignes.push("Stripe : le compte se lit de nouveau.".to_string());
                }
                let periode = if self.deja_lu { "" } else { " ; relu toutes les 10 min" };
                let duree = entreprise::secondes(duree);
                lignes.push(format!("Stripe : lu en {duree} ({requetes} requêtes GET){periode}."));
                self.en_panne = false;
                self.deja_lu = true;
                self.reprise = REPRISE;
                self.prochaine = maintenant + PERIODE;
            }
            Err(e) => {
                if !self.en_panne {
                    let minutes = self.reprise.as_secs().div_ceil(60);
                    lignes.push(format!(
                        "Stripe : {e} — nouvel essai dans {minutes} min (une seule ligne jusqu'au retour)."
                    ));
                    self.en_panne = true;
                }
                self.prochaine = maintenant + self.reprise;
                self.reprise = (self.reprise * 2).min(PERIODE);
            }
        }
        lignes
    }
}

// ---- Une lecture ----------------------------------------------------------------------------------------------------

/// Les éléments d'une page de liste (le plus que permet Stripe), et les pages qu'une lecture parcourt au plus : au-delà
/// de 1 000 transactions dans le mois, ou de factures ouvertes, le compte dit « au moins » (`plus`) ; au-delà de 1 000
/// abonnements actifs, les 1 000 premiers.
const PAR_PAGE: usize = 100;
const PAGES_MAX: usize = 10;
/// Les derniers paiements que montre le tableau.
const PAIEMENTS: usize = 5;
/// Les virements lus pour trouver le prochain : ceux en route sont les plus récents.
const VIREMENTS: usize = 10;
/// Ce qui se montre d'un nom de client, au plus.
const NOM_MAX: usize = 80;
/// Les catégories de transactions du solde qui font ce qui est entré : les paiements (`charge` : les types `charge` et
/// `payment`), et ce qui les défait ou les refait — les remboursements (`refund` : `refund` et `payment_refund`), la
/// part qu'une capture partielle n'a pas prise, les paiements qui échouent après coup, les remboursements qui
/// échouent. Les frais, les virements, les réserves n'y sont pas.
const ENTREES: [&str; 5] = ["charge", "refund", "partial_capture_reversal", "charge_failure", "refund_failure"];

/// Pourquoi une lecture n'a pas abouti.
#[derive(Clone, Debug, PartialEq)]
enum Panne {
    /// Rien ne se lira sans la personne : la clé refusée (401), un droit qui lui manque (403), une clé illisible. Les
    /// derniers montants ne restent pas : la raison se montre à leur place, tout de suite.
    Refus(String),
    /// Le réseau, Stripe qui flanche ou demande d'attendre, une réponse illisible : les derniers montants restent bons
    /// une heure, et une autre lecture vient bientôt.
    Passagere(String),
}

impl Panne {
    /// En une phrase, pour l'écran et le journal : jamais la clé, ni ce que Stripe dit du compte.
    fn texte(&self) -> &str {
        match self {
            Panne::Refus(t) | Panne::Passagere(t) => t,
        }
    }
}

/// Une lecture, tout de suite, avec cette clé : le début du jour et du mois à l'heure du poste, puis tout le compte.
fn lire_maintenant(cle: &str) -> Result<(Montants, usize), Panne> {
    let debuts = debuts(&entreprise::maintenant_iso())
        .ok_or_else(|| Panne::Passagere("l'heure du poste ne se lit pas".into()))?;
    lire_le_compte(&mut |url: &str| get(url, cle), debuts, secondes_unix(SystemTime::now()))
}

/// La seule requête vers Stripe : un GET, écrit en dur — Eve ne crée, ne change ni n'efface rien chez Stripe. Sur une
/// adresse que seule [`Lecture::page`] fait, d'une [`Route`] ; la clé en « Bearer », la version de l'API fixée.
fn get(url: &str, cle: &str) -> Result<(u16, Vec<u8>), String> {
    https::requete("GET", url, cle, &[("Stripe-Version", VERSION)], None, DELAI).map(|r| (r.statut, r.corps))
}

/// Ce qui fait une requête : le vrai GET ([`get`]), ou, dans les essais, un faux Stripe ; rend le code HTTP et le
/// corps de la réponse.
type Envoyer<'a> = dyn FnMut(&str) -> Result<(u16, Vec<u8>), String> + 'a;

/// Une lecture du compte en cours : qui fait les requêtes, combien sont parties, les droits que Stripe a refusés à la
/// clé (la lecture continue après un refus, pour les dire tous d'un coup).
struct Lecture<'a, 'b> {
    envoyer: &'a mut Envoyer<'b>,
    requetes: usize,
    refuses: Vec<String>,
}

impl Lecture<'_, '_> {
    /// La réponse d'une route, avec ces paramètres (déjà codés) ; rien quand Stripe refuse un droit à la clé.
    fn page(&mut self, route: Route, parametres: &str) -> Result<Option<Value>, Panne> {
        let separateur = if parametres.is_empty() { "" } else { "?" };
        let url = format!("{API}{}{separateur}{parametres}", route.chemin());
        self.requetes += 1;
        let (statut, corps) = (self.envoyer)(&url).map_err(Panne::Passagere)?;
        match statut {
            200..=299 => serde_json::from_slice(&corps).map(Some).map_err(|_| illisible(route)),
            401 => Err(Panne::Refus(CLE_REFUSEE.into())),
            403 => {
                for droit in droits_refuses(route, &corps) {
                    if !self.refuses.contains(&droit) {
                        self.refuses.push(droit);
                    }
                }
                Ok(None)
            }
            s => Err(Panne::Passagere(raison_http(s, &corps))),
        }
    }

    /// Les éléments d'une liste, page après page — 100 par page, chacune après le dernier de la précédente
    /// (`starting_after`), [`PAGES_MAX`] au plus — et s'il en restait au bout ; rien quand la clé n'a pas le droit de
    /// la lire. `filtres` : ceux de la liste (« status=active »), déjà codés.
    fn liste(&mut self, route: Route, filtres: &str) -> Result<Option<(Vec<Value>, bool)>, Panne> {
        let mut tous = Vec::new();
        let mut apres: Option<String> = None;
        for _ in 0..PAGES_MAX {
            let mut parametres = format!("{filtres}&limit={PAR_PAGE}");
            if let Some(id) = &apres {
                let _ = write!(parametres, "&starting_after={}", https::encoder(id));
            }
            let Some(mut page) = self.page(route, &parametres)? else { return Ok(None) };
            let encore = page.get("has_more").and_then(Value::as_bool) == Some(true);
            let Some(Value::Array(donnees)) = page.get_mut("data").map(Value::take) else {
                return Err(illisible(route));
            };
            apres = donnees.last().and_then(|d| d.get("id")).and_then(Value::as_str).map(str::to_string);
            tous.extend(donnees);
            if !encore || apres.is_none() {
                return Ok(Some((tous, false)));
            }
        }
        Ok(Some((tous, true)))
    }
}

/// Une lecture de tout le compte, route par route ([`Route::TOUTES`]) : `envoyer` fait chaque GET. `(minuit,
/// premier)` : le début du jour et celui du mois, et `maintenant`, en secondes Unix. Rend les montants et les requêtes
/// parties.
fn lire_le_compte(
    envoyer: &mut Envoyer<'_>,
    (minuit, premier): (i64, i64),
    maintenant: i64,
) -> Result<(Montants, usize), Panne> {
    let mut l = Lecture { envoyer, requetes: 0, refuses: Vec::new() };
    let solde = l.page(Route::Solde, "")?;
    let transactions = l.liste(Route::Transactions, &format!("created%5Bgte%5D={premier}"))?;
    let paiements = l.page(Route::Paiements, &format!("limit={PAIEMENTS}&expand%5B%5D=data.customer"))?;
    let abonnements = l.liste(Route::Abonnements, "status=active")?;
    let factures = l.liste(Route::Factures, "status=open")?;
    let virements = l.page(Route::Virements, &format!("limit={VIREMENTS}"))?;
    if !l.refuses.is_empty() {
        return Err(Panne::Refus(phrase_des_droits(&l.refuses)));
    }
    // Sans refus, chaque route a répondu.
    let (Some(solde), Some(transactions), Some(paiements), Some(abonnements), Some(factures), Some(virements)) =
        (solde, transactions, paiements, abonnements, factures, virements)
    else {
        return Err(Panne::Passagere("une réponse de Stripe manque".into()));
    };
    let (devise, disponible, en_attente) = solde_de(&solde).ok_or_else(|| illisible(Route::Solde))?;
    let ((transactions, tronquees), (factures, plus_de_factures)) = (transactions, factures);
    let (aujourdhui, ce_mois) = entrees(&transactions, tronquees, &devise, minuit);
    let montants = Montants {
        lu_a: maintenant,
        devise: devise.to_uppercase(),
        disponible,
        en_attente,
        aujourdhui,
        ce_mois,
        // Au-delà de [`PAGES_MAX`] pages, les premiers abonnements seulement (le contrat n'a pas de « au moins »).
        abonnements: Some(abonnements_de(&abonnements.0, &devise)),
        virement: virement_de(donnees(&virements, Route::Virements)?, &devise),
        ouvertes: Some(ouvertes_de(&factures, plus_de_factures, &devise, maintenant)),
        paiements: paiements_de(donnees(&paiements, Route::Paiements)?),
    };
    Ok((montants, l.requetes))
}

/// Les éléments d'une page de liste (`data`).
fn donnees(page: &Value, route: Route) -> Result<&[Value], Panne> {
    page.get("data").and_then(Value::as_array).map(Vec::as_slice).ok_or_else(|| illisible(route))
}

fn illisible(route: Route) -> Panne {
    Panne::Passagere(format!("une réponse ne se lit pas ({})", route.chemin()))
}

/// La devise du compte (celle de son solde, en minuscules comme Stripe l'écrit), son disponible et son en attente.
fn solde_de(solde: &Value) -> Option<(String, f64, f64)> {
    let fonds = |cle: &str| solde.get(cle).and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
    let devise = fonds("available").iter().chain(fonds("pending")).find_map(devise_de)?.to_ascii_lowercase();
    let total = |cle: &str| {
        let cents: i64 = fonds(cle)
            .iter()
            .filter(|f| devise_de(f) == Some(devise.as_str()))
            .filter_map(|f| f.get("amount")?.as_i64())
            .sum();
        en_unites(cents as f64, &devise)
    };
    let (disponible, en_attente) = (total("available"), total("pending"));
    Some((devise, disponible, en_attente))
}

/// Ce qui est entré depuis minuit et depuis le début du mois, d'après les transactions du solde lues (de la plus
/// récente à la plus ancienne, comme Stripe les rend), dans la devise du compte : leur montant brut, avant les frais de
/// Stripe ([`ENTREES`]) ; le nombre des paiements. `tronquees` : la lecture s'est arrêtée avant le début du mois — le
/// jour n'est incomplet que si la dernière transaction lue en est.
fn entrees(transactions: &[Value], tronquees: bool, devise: &str, minuit: i64) -> (Somme, Somme) {
    let (mut jour, mut mois) = ((0_i64, 0_usize), (0_i64, 0_usize));
    for t in transactions.iter().filter(|t| devise_de(t) == Some(devise)) {
        let categorie = t.get("reporting_category").and_then(Value::as_str).unwrap_or_default();
        let (Some(montant), Some(quand)) =
            (t.get("amount").and_then(Value::as_i64), t.get("created").and_then(Value::as_i64))
        else {
            continue;
        };
        if !ENTREES.contains(&categorie) {
            continue;
        }
        let paiement = usize::from(categorie == "charge");
        mois = (mois.0 + montant, mois.1 + paiement);
        if quand >= minuit {
            jour = (jour.0 + montant, jour.1 + paiement);
        }
    }
    let derniere = transactions.last().and_then(|t| t.get("created")?.as_i64());
    let somme = |(cents, nombre): (i64, usize), plus| Somme { montant: en_unites(cents as f64, devise), nombre, plus };
    (somme(jour, tronquees && derniere.is_some_and(|q| q >= minuit)), somme(mois, tronquees))
}

/// Les abonnements actifs : combien, et ce qu'ils rapportent par mois ([`mensuel_de`], article par article). Un
/// abonnement en pause (son encaissement suspendu) compte parmi les actifs, pas dans le revenu.
fn abonnements_de(abonnements: &[Value], devise: &str) -> Abonnements {
    let mensuel = abonnements
        .iter()
        .filter(|a| a.get("pause_collection").is_none_or(Value::is_null))
        .flat_map(|a| a.pointer("/items/data").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default())
        .filter_map(|article| mensuel_de(article, devise))
        .sum();
    Abonnements { actifs: abonnements.len(), mensuel }
}

/// Ce que rapporte par mois un article d'abonnement : son prix unitaire fois sa quantité, ramené au mois selon sa
/// période (un an en compte le douzième, une semaine les 365/12/7 de ses semaines, un jour 365/12). Rien pour un prix
/// d'une autre devise que le compte, au palier ou à l'usage (son montant ne se sait qu'à la facture) ; les remises (les
/// coupons) n'y sont pas ôtées.
fn mensuel_de(article: &Value, devise: &str) -> Option<f64> {
    let prix = article.get("price")?;
    let au_palier = !matches!(prix.get("billing_scheme").and_then(Value::as_str), None | Some("per_unit"));
    let periode = prix.get("recurring")?;
    let a_l_usage = periode.get("usage_type").and_then(Value::as_str) == Some("metered");
    if devise_de(prix)? != devise || au_palier || a_l_usage {
        return None;
    }
    let unitaire = match prix.get("unit_amount_decimal").and_then(Value::as_str) {
        Some(d) => d.trim().parse::<f64>().ok()?,
        None => prix.get("unit_amount")?.as_f64()?,
    };
    let quantite = article.get("quantity").and_then(Value::as_f64).unwrap_or(1.0);
    let n = periode.get("interval_count").and_then(Value::as_f64).filter(|n| *n >= 1.0).unwrap_or(1.0);
    let par_mois = match periode.get("interval")?.as_str()? {
        "day" => 365.0 / 12.0,
        "week" => 365.0 / 12.0 / 7.0,
        "month" => 1.0,
        "year" => 1.0 / 12.0,
        _ => return None,
    } / n;
    Some(en_unites(unitaire * quantite, devise) * par_mois)
}

/// Le prochain virement en route (en attente ou parti : celui qui arrive le plus tôt), sinon le dernier fait, dans la
/// devise du compte ; rien s'il n'y en a pas. Son arrivée, une date : Stripe la donne à minuit en temps universel.
fn virement_de(virements: &[Value], devise: &str) -> Option<Virement> {
    let du_compte: Vec<&Value> = virements.iter().filter(|v| devise_de(v) == Some(devise)).collect();
    let statut = |v: &Value| v.get("status").and_then(Value::as_str).unwrap_or_default().to_string();
    let arrivee = |v: &Value| v.get("arrival_date").and_then(Value::as_i64);
    let en_route = du_compte
        .iter()
        .filter(|v| matches!(statut(v).as_str(), "pending" | "in_transit"))
        .filter_map(|v| Some((arrivee(v)?, *v)))
        .min_by_key(|(quand, _)| *quand)
        .map(|(_, v)| v);
    let (v, fait) = match en_route {
        Some(v) => (v, false),
        None => (*du_compte.iter().find(|v| statut(v) == "paid")?, true),
    };
    Some(Virement {
        montant: en_unites(v.get("amount")?.as_i64()? as f64, devise),
        arrivee: centre::date_du_numero(arrivee(v)?.div_euclid(86_400)),
        fait,
    })
}

/// Les factures ouvertes : combien, leur total dû (`amount_remaining`, dans la devise du compte), combien sont échues
/// — leur échéance passée ; ou, payées d'office (sans échéance : `charge_automatically`), quand un paiement a été tenté
/// sans les régler (un prélèvement bancaire en cours, quelques jours, y compte aussi).
fn ouvertes_de(factures: &[Value], plus: bool, devise: &str, maintenant: i64) -> Ouvertes {
    let echue = |f: &&Value| match f.get("due_date").and_then(Value::as_i64) {
        Some(echeance) => echeance < maintenant,
        None => {
            f.get("collection_method").and_then(Value::as_str) == Some("charge_automatically")
                && f.get("attempted").and_then(Value::as_bool) == Some(true)
        }
    };
    let du: i64 = factures
        .iter()
        .filter(|f| devise_de(f) == Some(devise))
        .filter_map(|f| f.get("amount_remaining")?.as_i64())
        .sum();
    Ouvertes {
        nombre: factures.len(),
        plus,
        du: en_unites(du as f64, devise),
        echues: factures.iter().filter(echue).count(),
    }
}

/// Les derniers paiements, le plus récent d'abord (comme Stripe les rend), [`PAIEMENTS`] au plus.
fn paiements_de(charges: &[Value]) -> Vec<Paiement> {
    let paiement = |c: &Value| {
        Some(Paiement {
            montant: en_unites(c.get("amount")?.as_i64()? as f64, devise_de(c)?),
            quand: c.get("created")?.as_i64()?,
            qui: qui_a_paye(c),
            issue: issue_de(c),
        })
    };
    charges.iter().filter_map(paiement).take(PAIEMENTS).collect()
}

/// L'issue d'un paiement : échoué, en attente (un prélèvement bancaire en cours, une carte autorisée mais pas encore
/// débitée), remboursé en entier, sinon réussi (un remboursement partiel le laisse réussi).
fn issue_de(c: &Value) -> Issue {
    let drapeau = |cle: &str| c.get(cle).and_then(Value::as_bool);
    match c.get("status").and_then(Value::as_str) {
        Some("failed") => Issue::Echoue,
        Some("pending") => Issue::EnAttente,
        _ if drapeau("refunded") == Some(true) => Issue::Rembourse,
        _ if drapeau("captured") == Some(false) => Issue::EnAttente,
        _ => Issue::Reussi,
    }
}

/// Qui a payé : le nom du client (lu avec le paiement : `expand[]=data.customer`), sinon son courriel, sinon le nom ou
/// le courriel de la carte, sinon la description du paiement ; sur une ligne, courte ; vide si Stripe n'en dit rien.
fn qui_a_paye(c: &Value) -> String {
    let client = c.get("customer").filter(|v| v.is_object());
    let champs = [
        client.and_then(|v| v.get("name")),
        client.and_then(|v| v.get("email")),
        c.pointer("/billing_details/name"),
        c.pointer("/billing_details/email"),
        c.get("receipt_email"),
        c.get("description"),
    ];
    champs.into_iter().flatten().filter_map(Value::as_str).map(propre).find(|t| !t.is_empty()).unwrap_or_default()
}

/// Un texte de Stripe pour l'écran : sur une ligne (ses blancs, retours de ligne compris, réduits à une espace), sans
/// caractère de contrôle, [`NOM_MAX`] caractères au plus.
fn propre(s: &str) -> String {
    let ligne: String =
        s.split_whitespace().collect::<Vec<_>>().join(" ").chars().filter(|c| !c.is_control()).collect();
    if ligne.chars().count() <= NOM_MAX {
        return ligne;
    }
    let mut court: String = ligne.chars().take(NOM_MAX - 1).collect();
    court.push('…');
    court
}

/// La devise d'un objet de Stripe (« cad »).
fn devise_de(v: &Value) -> Option<&str> {
    v.get("currency")?.as_str()
}

/// Un montant de Stripe (dans la plus petite unité de sa devise : les cents) dans l'unité de la devise : divisé par
/// 100, sauf pour les devises sans décimales (le yen, le won…) et celles à trois (le dinar du Koweït…), comme Stripe
/// les compte.
fn en_unites(montant: f64, devise: &str) -> f64 {
    const SANS_DECIMALES: [&str; 16] = [
        "bif", "clp", "djf", "gnf", "jpy", "kmf", "krw", "mga", "pyg", "rwf", "ugx", "vnd", "vuv", "xaf", "xof", "xpf",
    ];
    const TROIS_DECIMALES: [&str; 5] = ["bhd", "jod", "kwd", "omr", "tnd"];
    let d = devise.to_ascii_lowercase();
    if SANS_DECIMALES.contains(&d.as_str()) {
        montant
    } else if TROIS_DECIMALES.contains(&d.as_str()) {
        montant / 1000.0
    } else {
        montant / 100.0
    }
}

/// Les droits que Stripe dit manquer à la clé (« … Having the 'rak_invoice_read' permission would allow this request
/// to continue. ») ; s'il n'en nomme aucun, ceux que demande la route. Seuls ces noms sortent de son message, qui cite
/// aussi la clé (masquée) et le compte.
fn droits_refuses(route: Route, corps: &[u8]) -> Vec<String> {
    let message = serde_json::from_slice::<Value>(corps)
        .ok()
        .and_then(|v| Some(v.pointer("/error/message")?.as_str()?.to_string()))
        .unwrap_or_default();
    let nom = |code: String| DROITS.iter().find(|d| d.code == code).map_or(code, |d| d.nom.to_string());
    let nommes: Vec<String> = codes_rak(&message).into_iter().map(nom).collect();
    if nommes.is_empty() { route.droits().iter().map(|d| d.nom.to_string()).collect() } else { nommes }
}

/// Les codes de droits que nomme un refus de Stripe (« rak_invoice_read »), chacun une fois, dans l'ordre.
fn codes_rak(message: &str) -> Vec<String> {
    let mut codes: Vec<String> = Vec::new();
    for (i, _) in message.match_indices("rak_") {
        let code: String = message[i..].chars().take_while(|c| c.is_ascii_lowercase() || *c == '_').collect();
        if code.len() > "rak_".len() && !codes.contains(&code) {
            codes.push(code);
        }
    }
    codes
}

/// Ce qui se dit des droits qui manquent à la clé, et où les lui donner.
fn phrase_des_droits(noms: &[String]) -> String {
    let liste = match noms {
        [debut @ .., dernier] if !debut.is_empty() => format!("{} et {dernier}", debut.join(", ")),
        _ => noms.join(""),
    };
    format!(
        "la clé d'API n'a pas le droit de lire {liste} : à lui donner en lecture (« Read »), dans Stripe \
         (Développeurs › Clés API)"
    )
}

/// Ce qui se dit d'une réponse que la lecture n'attendait pas : son code, et ce que Stripe en dit de sûr — la sorte
/// d'erreur et le paramètre en cause, jamais son message (il peut citer un identifiant du compte).
fn raison_http(statut: u16, corps: &[u8]) -> String {
    match statut {
        429 => "trop de requêtes : l'API demande d'attendre (HTTP 429)".into(),
        500..=599 => format!("l'API ne répond pas comme il faut (HTTP {statut})"),
        s => {
            let erreur = serde_json::from_slice::<Value>(corps).ok();
            let champ = |c: &str| erreur.as_ref()?.get("error")?.get(c)?.as_str().map(propre);
            let detail: Vec<String> = [champ("code"), champ("param")].into_iter().flatten().collect();
            let detail = if detail.is_empty() { String::new() } else { format!(" : {}", detail.join(", ")) };
            format!("l'API refuse la requête d'Eve (HTTP {s}{detail})")
        }
    }
}

// ---- L'heure --------------------------------------------------------------------------------------------------------

/// Le début du jour et celui du mois, à l'heure du poste, en secondes Unix, d'après l'heure du poste en ISO 8601
/// (`entreprise::maintenant_iso` : « 2026-10-27T09:12:00-04:00 »). Le décalage est celui de maintenant : passé un
/// changement d'heure, le début du jour où il tombe, et celui du mois, glissent d'une heure (une heure de paiements).
fn debuts(iso: &str) -> Option<(i64, i64)> {
    let decalage = iso.get(19..)?;
    let minuit = entreprise::en_unix(&format!("{}T00:00:00{decalage}", iso.get(..10)?))?;
    let premier = entreprise::en_unix(&format!("{}-01T00:00:00{decalage}", iso.get(..7)?))?;
    Some((minuit, premier))
}

fn secondes_unix(t: SystemTime) -> i64 {
    t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

#[cfg(test)]
mod essais {
    use serde_json::json;

    use super::*;

    /// Le 15 janvier 2027 à 15 h, à l'heure de l'Est (UTC-5) : l'heure des essais.
    const MAINTENANT: &str = "2027-01-15T15:00:00-05:00";

    fn unix(iso: &str) -> i64 {
        entreprise::en_unix(iso).unwrap()
    }

    fn debuts_de_l_essai() -> (i64, i64) {
        debuts(MAINTENANT).unwrap()
    }

    /// Une liste de Stripe, de la forme qu'il rend (relevée le 27 octobre 2026).
    fn liste(chemin: &str, donnees: Vec<Value>, encore: bool) -> Value {
        json!({ "object": "list", "data": donnees, "has_more": encore, "url": chemin })
    }

    /// Une transaction du solde, de la forme que rend Stripe, aux valeurs inventées.
    fn transaction(id: &str, categorie: &str, montant: i64, devise: &str, quand: &str) -> Value {
        let sorte = match categorie {
            "fee" => "stripe_fee",
            "charge_failure" => "payment_failure_refund",
            c => c,
        };
        json!({
            "id": id, "object": "balance_transaction", "amount": montant, "available_on": unix(quand) + 172_800,
            "balance_type": "payments", "created": unix(quand), "currency": devise, "description": null,
            "exchange_rate": null, "fee": 0, "fee_details": [], "net": montant, "reporting_category": categorie,
            "source": null, "status": "pending", "type": sorte
        })
    }

    /// Un article d'abonnement : son prix, sa période, sa quantité.
    fn article(montant: Value, devise: &str, periode: &str, nombre: u32, quantite: u32) -> Value {
        json!({
            "id": "si_essai", "object": "subscription_item", "quantity": quantite,
            "price": {
                "id": "price_essai", "object": "price", "billing_scheme": "per_unit", "currency": devise,
                "unit_amount": montant, "unit_amount_decimal": montant.as_i64().map(|m| m.to_string()),
                "recurring": { "interval": periode, "interval_count": nombre, "usage_type": "licensed" },
                "type": "recurring"
            }
        })
    }

    fn abonnement(id: &str, articles: Vec<Value>, pause: Value) -> Value {
        json!({
            "id": id, "object": "subscription", "status": "active", "currency": "cad", "customer": "cus_essai",
            "pause_collection": pause, "items": { "object": "list", "data": articles, "has_more": false }
        })
    }

    fn facture(id: &str, methode: &str, echeance: Option<i64>, tentee: bool, reste: i64, devise: &str) -> Value {
        json!({
            "id": id, "object": "invoice", "status": "open", "collection_method": methode, "due_date": echeance,
            "attempted": tentee, "amount_due": reste, "amount_remaining": reste, "currency": devise,
            "customer_name": "Construction Exemple inc."
        })
    }

    fn virement(id: &str, statut: &str, montant: i64, arrivee: &str) -> Value {
        json!({
            "id": id, "object": "payout", "amount": montant, "arrival_date": unix(arrivee), "automatic": true,
            "created": unix(arrivee) - 259_200, "currency": "cad", "method": "standard", "status": statut,
            "type": "bank_account"
        })
    }

    /// Le client d'un paiement, tel que le rend `expand[]=data.customer`.
    fn client(nom: Value, courriel: Value) -> Value {
        json!({ "id": "cus_essai", "object": "customer", "name": nom, "email": courriel })
    }

    fn paiement(id: &str, montant: i64, statut: &str, client: Value, plus: Value) -> Value {
        let mut c = json!({
            "id": id, "object": "charge", "amount": montant, "amount_refunded": 0, "captured": true,
            "created": unix("2027-01-15T14:00:00-05:00"), "currency": "cad", "customer": client, "description": null,
            "billing_details": { "name": null, "email": null }, "paid": statut == "succeeded", "refunded": false,
            "receipt_email": null, "status": statut
        });
        for (cle, valeur) in plus.as_object().into_iter().flatten() {
            c[cle] = valeur.clone();
        }
        c
    }

    /// Les réponses d'un compte inventé, de la forme des vraies : une entreprise de construction, ses abonnés.
    fn reponse_du_compte(url: &str) -> (u16, Value) {
        let reste = url.strip_prefix(API).unwrap();
        let (chemin, requete) = reste.split_once('?').unwrap_or((reste, ""));
        let corps = match chemin {
            "/v1/balance" => json!({
                "object": "balance", "livemode": false,
                "available": [
                    { "amount": 1_234_567, "currency": "cad", "source_types": { "card": 1_234_567 } },
                    { "amount": 5_000, "currency": "usd", "source_types": { "card": 5_000 } }
                ],
                "pending": [{ "amount": 89_012, "currency": "cad", "source_types": { "card": 89_012 } }],
                "instant_available": [{ "amount": 0, "currency": "cad" }]
            }),
            "/v1/balance_transactions" if !requete.contains("starting_after") => liste(
                chemin,
                vec![
                    transaction("txn_essai1", "charge", 15_000, "cad", "2027-01-15T14:00:00-05:00"),
                    transaction("txn_essai2", "fee", -435, "cad", "2027-01-15T14:00:00-05:00"),
                    transaction("txn_essai3", "refund", -5_000, "cad", "2027-01-15T09:00:00-05:00"),
                ],
                true,
            ),
            "/v1/balance_transactions" => liste(
                chemin,
                vec![
                    transaction("txn_essai4", "charge", 20_000, "cad", "2027-01-14T10:00:00-05:00"),
                    transaction("txn_essai5", "payout", -100_000, "cad", "2027-01-10T10:00:00-05:00"),
                    transaction("txn_essai6", "charge", 7_500, "usd", "2027-01-09T10:00:00-05:00"),
                    transaction("txn_essai7", "refund_failure", 1_000, "cad", "2027-01-05T10:00:00-05:00"),
                    transaction("txn_essai8", "charge_failure", -2_000, "cad", "2027-01-03T10:00:00-05:00"),
                    transaction("txn_essai9", "charge", 30_000, "cad", "2027-01-02T10:00:00-05:00"),
                    transaction("txn_essai10", "partial_capture_reversal", -500, "cad", "2027-01-02T10:00:00-05:00"),
                    transaction(
                        "txn_essai11", "payout_minimum_balance_hold", -1_000, "cad", "2027-01-01T10:00:00-05:00",
                    ),
                ],
                false,
            ),
            "/v1/charges" => liste(
                chemin,
                vec![
                    paiement(
                        "ch_essai1",
                        15_000,
                        "succeeded",
                        client(json!("Construction Exemple inc."), json!("compta@exemple.ca")),
                        json!({}),
                    ),
                    paiement("ch_essai2", 4_900, "failed", client(Value::Null, json!("sam@exemple.ca")), json!({})),
                    paiement(
                        "ch_essai3",
                        5_000,
                        "succeeded",
                        json!("cus_essai3"),
                        json!({
                            "refunded": true, "amount_refunded": 5_000,
                            "billing_details": { "name": "Sam Exemple" }
                        }),
                    ),
                    paiement(
                        "ch_essai4",
                        20_000,
                        "pending",
                        json!({ "id": "cus_essai4", "object": "customer", "deleted": true }),
                        json!({ "description": "Facture 0042" }),
                    ),
                    paiement(
                        "ch_essai5",
                        30_000,
                        "succeeded",
                        Value::Null,
                        json!({ "captured": false, "description": "Abonnement\n  mensuel" }),
                    ),
                    paiement("ch_essai6", 1_000, "succeeded", Value::Null, json!({})),
                ],
                true,
            ),
            "/v1/subscriptions" => liste(
                chemin,
                vec![
                    abonnement("sub_essai1", vec![article(json!(9_900), "cad", "month", 1, 1)], Value::Null),
                    abonnement(
                        "sub_essai2",
                        vec![article(json!(120_000), "cad", "year", 1, 1), article(json!(500), "cad", "month", 1, 3)],
                        Value::Null,
                    ),
                    abonnement(
                        "sub_essai3",
                        vec![article(json!(5_000), "cad", "month", 1, 1)],
                        json!({ "behavior": "void" }),
                    ),
                    abonnement("sub_essai4", vec![article(json!(2_000), "usd", "month", 1, 1)], Value::Null),
                    {
                        let mut a = article(json!(10), "cad", "month", 1, 1);
                        a["price"]["recurring"]["usage_type"] = json!("metered");
                        abonnement("sub_essai5", vec![a], Value::Null)
                    },
                ],
                false,
            ),
            "/v1/invoices" => {
                let jour = 86_400;
                let maintenant = unix(MAINTENANT);
                liste(
                    chemin,
                    vec![
                        facture("in_essai1", "send_invoice", Some(maintenant - 3 * jour), false, 45_000, "cad"),
                        facture("in_essai2", "send_invoice", Some(maintenant + 10 * jour), false, 12_000, "cad"),
                        facture("in_essai3", "charge_automatically", None, true, 9_900, "cad"),
                        facture("in_essai4", "charge_automatically", None, false, 9_900, "cad"),
                        facture("in_essai5", "send_invoice", Some(maintenant - jour), false, 5_000, "usd"),
                    ],
                    false,
                )
            }
            "/v1/payouts" => liste(
                chemin,
                vec![
                    virement("po_essai1", "pending", 80_000, "2027-01-20T00:00:00Z"),
                    virement("po_essai2", "in_transit", 150_000, "2027-01-18T00:00:00Z"),
                    virement("po_essai3", "paid", 99_000, "2027-01-13T00:00:00Z"),
                    virement("po_essai4", "failed", 1_000, "2027-01-12T00:00:00Z"),
                ],
                true,
            ),
            _ => return (404, json!({ "error": { "type": "invalid_request_error", "code": "resource_missing" } })),
        };
        (200, corps)
    }

    /// Une lecture sur un faux Stripe (`repondre` : le code et le corps de chaque adresse) : son résultat, et les
    /// adresses demandées.
    fn lire_avec(repondre: impl Fn(&str) -> (u16, Value)) -> (Result<(Montants, usize), Panne>, Vec<String>) {
        let mut demandes = Vec::new();
        let lu = {
            let mut envoyer = |url: &str| -> Result<(u16, Vec<u8>), String> {
                demandes.push(url.to_string());
                let (statut, corps) = repondre(url);
                Ok((statut, corps.to_string().into_bytes()))
            };
            lire_le_compte(&mut envoyer, debuts_de_l_essai(), unix(MAINTENANT))
        };
        (lu, demandes)
    }

    fn proche(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn les_routes_sont_des_lectures_nommees_une_a_une() {
        let chemins: Vec<&str> = Route::TOUTES.iter().map(|r| r.chemin()).collect();
        assert_eq!(
            chemins,
            [
                "/v1/balance", "/v1/balance_transactions", "/v1/charges", "/v1/subscriptions", "/v1/invoices",
                "/v1/payouts"
            ]
        );
        // Les droits des routes sont ceux d'une clé restreinte pour Eve, chacun une fois, tous en lecture.
        let mut droits: Vec<Droit> = Vec::new();
        for d in Route::TOUTES.iter().flat_map(|r| r.droits()) {
            if !droits.contains(d) {
                droits.push(*d);
            }
        }
        assert_eq!(droits, DROITS);
        assert!(DROITS.iter().all(|d| d.code.starts_with("rak_") && d.code.ends_with("_read")), "{DROITS:?}");
    }

    #[test]
    fn une_lecture_du_compte_entier() {
        let (lu, demandes) = lire_avec(reponse_du_compte);
        let (m, requetes) = lu.unwrap();
        let (_, premier) = debuts_de_l_essai();
        let attendues = [
            "https://api.stripe.com/v1/balance".to_string(),
            format!("https://api.stripe.com/v1/balance_transactions?created%5Bgte%5D={premier}&limit=100"),
            format!(
                "https://api.stripe.com/v1/balance_transactions?created%5Bgte%5D={premier}&limit=100\
                 &starting_after=txn_essai3"
            ),
            "https://api.stripe.com/v1/charges?limit=5&expand%5B%5D=data.customer".to_string(),
            "https://api.stripe.com/v1/subscriptions?status=active&limit=100".to_string(),
            "https://api.stripe.com/v1/invoices?status=open&limit=100".to_string(),
            "https://api.stripe.com/v1/payouts?limit=10".to_string(),
        ];
        assert_eq!(demandes, attendues);
        assert_eq!(requetes, 7);
        assert_eq!((m.lu_a, m.devise.as_str()), (unix(MAINTENANT), "CAD"));
        // Le solde de la devise du compte : celui en dollars américains n'y entre pas.
        assert!(proche(m.disponible, 12_345.67) && proche(m.en_attente, 890.12), "{m:?}");
        // Aujourd'hui : 150 $ payés, 50 $ remboursés ; les frais ne comptent pas.
        assert_eq!((m.aujourdhui.nombre, m.aujourdhui.plus), (1, false));
        assert!(proche(m.aujourdhui.montant, 100.0), "{:?}", m.aujourdhui);
        // Le mois : trois paiements, le remboursement, celui qui a échoué, le paiement qui a échoué après coup, la part
        // d'une capture partielle ; ni le virement, ni la réserve, ni le paiement en dollars américains.
        assert_eq!((m.ce_mois.nombre, m.ce_mois.plus), (3, false));
        assert!(proche(m.ce_mois.montant, 585.0), "{:?}", m.ce_mois);
        // 99 $ par mois, 1 200 $ par année (100 $ par mois) et trois fois 5 $ ; ni la pause, ni une autre devise, ni
        // un prix à l'usage.
        let a = m.abonnements.unwrap();
        assert_eq!(a.actifs, 5);
        assert!(proche(a.mensuel, 214.0), "{a:?}");
        // Le prochain virement : celui qui arrive le plus tôt, parmi ceux en route.
        assert_eq!(m.virement, Some(Virement { montant: 1_500.0, arrivee: "2027-01-18".into(), fait: false }));
        // Cinq factures ouvertes : deux échues d'après leur échéance, une payée d'office dont le paiement a échoué.
        let o = m.ouvertes.unwrap();
        assert_eq!((o.nombre, o.plus, o.echues), (5, false, 3));
        assert!(proche(o.du, 768.0), "{o:?}");
        // Les cinq derniers paiements, le plus récent d'abord.
        let vus: Vec<(f64, &str, Issue)> = m.paiements.iter().map(|p| (p.montant, p.qui.as_str(), p.issue)).collect();
        assert_eq!(
            vus,
            [
                (150.0, "Construction Exemple inc.", Issue::Reussi),
                (49.0, "sam@exemple.ca", Issue::Echoue),
                (50.0, "Sam Exemple", Issue::Rembourse),
                (200.0, "Facture 0042", Issue::EnAttente),
                (300.0, "Abonnement mensuel", Issue::EnAttente),
            ]
        );
        assert_eq!(m.paiements[0].quand, unix("2027-01-15T14:00:00-05:00"));
    }

    #[test]
    fn la_cle_ne_part_jamais_dans_une_adresse() {
        let (_, demandes) = lire_avec(reponse_du_compte);
        for url in &demandes {
            assert!(url.starts_with("https://api.stripe.com/v1/"), "{url}");
            assert!(!url.contains("sk_") && !url.contains("rk_") && !url.contains("key"), "{url}");
        }
    }

    #[test]
    fn une_cle_refusee_arrete_tout() {
        let message = "Invalid API Key provided: sk_live_****AbCd";
        let refus = json!({ "error": { "type": "invalid_request_error", "message": message } });
        let (lu, demandes) = lire_avec(|_| (401, refus.clone()));
        assert_eq!(lu.unwrap_err(), Panne::Refus(CLE_REFUSEE.into()));
        assert_eq!(demandes.len(), 1, "une seule requête : la clé refusée ne se réessaie pas route par route");
    }

    #[test]
    fn les_droits_qui_manquent_se_disent_tous() {
        let refus = |code: &str| {
            json!({ "error": { "type": "invalid_request_error", "message": format!(
                "The provided key 'rk_live_*********************Ab12' does not have the required permissions for this \
                 endpoint on account 'acct_1Exemple'. Having the '{code}' permission would allow this request to \
                 continue."
            ) } })
        };
        let (lu, demandes) = lire_avec(|url| match url.strip_prefix(API).unwrap().split('?').next().unwrap() {
            "/v1/subscriptions" => (403, refus("rak_subscription_read")),
            "/v1/invoices" => (403, refus("rak_invoice_read")),
            // Un refus qui ne nomme pas son droit : ceux que la route demande.
            "/v1/charges" => (403, json!({ "error": { "message": "Forbidden" } })),
            _ => reponse_du_compte(url),
        });
        let Err(Panne::Refus(phrase)) = lu else { panic!("{lu:?}") };
        assert!(phrase.contains("lire Charges, Customers, Subscriptions et Invoices :"), "{phrase}");
        assert!(phrase.contains("« Read »"), "{phrase}");
        for secret in ["acct_", "rk_live", "Ab12", "rak_"] {
            assert!(!phrase.contains(secret), "{phrase}");
        }
        // Toutes les routes ont été essayées : les droits se disent d'un coup.
        assert_eq!(demandes.len(), 7, "{demandes:#?}");
    }

    #[test]
    fn une_panne_en_passant() {
        let essai = |statut: u16, corps: Value| {
            let repondre = move |url: &str| match url.contains("/v1/invoices") {
                true => (statut, corps.clone()),
                false => reponse_du_compte(url),
            };
            lire_avec(repondre).0
        };
        let attendu = |t: &str| -> Result<(Montants, usize), Panne> { Err(Panne::Passagere(t.into())) };
        assert_eq!(essai(503, json!({})), attendu("l'API ne répond pas comme il faut (HTTP 503)"));
        assert_eq!(essai(429, json!({})), attendu("trop de requêtes : l'API demande d'attendre (HTTP 429)"));
        // Une requête refusée dit la sorte de l'erreur et le paramètre, jamais le message (il cite le compte).
        let e = json!({ "error": { "code": "parameter_unknown", "param": "status", "message": "acct_1Exemple : x" } });
        assert_eq!(essai(400, e), attendu("l'API refuse la requête d'Eve (HTTP 400 : parameter_unknown, status)"));
        assert_eq!(essai(404, Value::Null), attendu("l'API refuse la requête d'Eve (HTTP 404)"));
        // Le réseau ; une réponse qui n'est pas du JSON, ou sans sa liste.
        let mut sans_reseau = |_: &str| -> Result<(u16, Vec<u8>), String> {
            Err("le nom du serveur est introuvable (pas de réseau ?)".to_string())
        };
        let lu = lire_le_compte(&mut sans_reseau, debuts_de_l_essai(), unix(MAINTENANT));
        assert_eq!(lu.unwrap_err(), Panne::Passagere("le nom du serveur est introuvable (pas de réseau ?)".into()));
        let mut pas_du_json = |url: &str| -> Result<(u16, Vec<u8>), String> {
            match url.contains("/v1/charges") {
                true => Ok((200, b"<html>".to_vec())),
                false => Ok((200, reponse_du_compte(url).1.to_string().into_bytes())),
            }
        };
        let lu = lire_le_compte(&mut pas_du_json, debuts_de_l_essai(), unix(MAINTENANT));
        assert_eq!(lu.unwrap_err(), Panne::Passagere("une réponse ne se lit pas (/v1/charges)".into()));
        assert_eq!(essai(200, json!({ "object": "list" })), attendu("une réponse ne se lit pas (/v1/invoices)"));
    }

    #[test]
    fn une_liste_s_arrete_a_dix_pages() {
        // Un mois qui n'en finit pas : chaque page, cent transactions d'aujourd'hui, et d'autres encore.
        let page = |url: &str| -> (u16, Value) {
            if !url.contains("/v1/balance_transactions") {
                return reponse_du_compte(url);
            }
            // La page qui suit celle de la dernière transaction lue (« txn_p03_099 » : la quatrième).
            let n = url.split("starting_after=txn_p").nth(1).map_or(0, |r| r[..2].parse::<usize>().unwrap() + 1);
            let heure = "2027-01-15T12:00:00-05:00";
            let donnees = (0..100).map(|i| transaction(&format!("txn_p{n:02}_{i:03}"), "charge", 100, "cad", heure));
            (200, liste("/v1/balance_transactions", donnees.collect(), true))
        };
        let (lu, demandes) = lire_avec(page);
        let (m, _) = lu.unwrap();
        assert_eq!(demandes.iter().filter(|u| u.contains("/v1/balance_transactions")).count(), PAGES_MAX);
        assert_eq!((m.ce_mois.nombre, m.ce_mois.plus), (1_000, true));
        // La dernière transaction lue est d'aujourd'hui : le jour aussi est « au moins ».
        assert_eq!((m.aujourdhui.nombre, m.aujourdhui.plus), (1_000, true));
        assert!(proche(m.ce_mois.montant, 1_000.0));
    }

    #[test]
    fn le_jour_est_entier_quand_la_lecture_s_arrete_avant() {
        let (minuit, _) = debuts_de_l_essai();
        let lues = [
            transaction("txn_a", "charge", 1_000, "cad", "2027-01-15T10:00:00-05:00"),
            transaction("txn_b", "charge", 2_000, "cad", "2027-01-14T10:00:00-05:00"),
        ];
        let (jour, mois) = entrees(&lues, true, "cad", minuit);
        assert_eq!((jour.nombre, jour.plus, mois.nombre, mois.plus), (1, false, 2, true));
        let (jour, _) = entrees(&lues[..1], true, "cad", minuit);
        assert!(jour.plus, "la dernière lue est d'aujourd'hui");
        // Un jour sans paiement, et rien du tout.
        assert_eq!(entrees(&lues[1..], false, "cad", minuit).0, Somme::default());
        assert_eq!(entrees(&[], false, "cad", minuit), (Somme::default(), Somme::default()));
    }

    #[test]
    fn le_revenu_mensuel_d_un_article() {
        let m = |montant: Value, periode: &str, nombre: u32, quantite: u32| {
            mensuel_de(&article(montant, "cad", periode, nombre, quantite), "cad")
        };
        assert!(proche(m(json!(1_000), "month", 1, 1).unwrap(), 10.0));
        assert!(proche(m(json!(3_000), "month", 3, 1).unwrap(), 10.0), "un trimestre");
        assert!(proche(m(json!(12_000), "year", 1, 1).unwrap(), 10.0));
        assert!(proche(m(json!(24_000), "year", 2, 1).unwrap(), 10.0));
        assert!(proche(m(json!(250), "month", 1, 4).unwrap(), 10.0), "quatre places");
        assert!(proche(m(json!(1_000), "week", 1, 1).unwrap(), 10.0 * 365.0 / 84.0));
        assert!(proche(m(json!(100), "day", 1, 1).unwrap(), 365.0 / 12.0));
        // Un prix en fraction de cent.
        let mut fin = article(json!(1_234), "cad", "month", 1, 1);
        fin["price"]["unit_amount_decimal"] = json!("1234.5");
        assert!(proche(mensuel_de(&fin, "cad").unwrap(), 12.345));
        // Au palier, à l'usage, une autre devise, une période inconnue : rien.
        let mut palier = article(Value::Null, "cad", "month", 1, 1);
        palier["price"]["billing_scheme"] = json!("tiered");
        assert_eq!(mensuel_de(&palier, "cad"), None);
        let mut usage = article(json!(10), "cad", "month", 1, 1);
        usage["price"]["recurring"]["usage_type"] = json!("metered");
        assert_eq!(mensuel_de(&usage, "cad"), None);
        assert_eq!(mensuel_de(&article(json!(1_000), "usd", "month", 1, 1), "cad"), None);
        assert_eq!(m(json!(1_000), "fortnight", 1, 1), None);
    }

    #[test]
    fn le_virement_qui_vient_sinon_le_dernier_fait() {
        let fait = [
            virement("po_1", "failed", 1_000, "2027-01-14T00:00:00Z"),
            virement("po_2", "paid", 99_000, "2027-01-13T00:00:00Z"),
            virement("po_3", "paid", 50_000, "2027-01-10T00:00:00Z"),
        ];
        assert_eq!(
            virement_de(&fait, "cad"),
            Some(Virement { montant: 990.0, arrivee: "2027-01-13".into(), fait: true })
        );
        assert_eq!(virement_de(&fait[..1], "cad"), None, "un virement manqué n'est pas un virement fait");
        assert_eq!(virement_de(&[], "cad"), None);
        // L'arrivée est une date en temps universel : minuit à Greenwich, c'est la veille au soir à Montréal.
        let en_route = [virement("po_4", "in_transit", 12_345, "2027-01-18T00:00:00Z")];
        assert_eq!(virement_de(&en_route, "cad").unwrap().arrivee, "2027-01-18");
    }

    #[test]
    fn les_derniers_paiements_et_qui_a_paye() {
        let long = "Construction Exemple inc. ".repeat(5);
        let c = paiement(
            "ch_x",
            1_000,
            "succeeded",
            json!({ "object": "customer", "name": long, "email": null }),
            json!({}),
        );
        let qui = qui_a_paye(&c);
        assert_eq!(qui.chars().count(), NOM_MAX);
        assert!(qui.ends_with('…') && qui.starts_with("Construction Exemple inc. Construction"), "{qui}");
        // Le nom de la carte, puis le courriel du reçu ; un nom vide passe au suivant.
        let carte = paiement(
            "ch_y",
            1,
            "succeeded",
            json!({ "object": "customer", "name": " ", "email": null }),
            json!({ "billing_details": { "name": null, "email": "sam@exemple.ca" } }),
        );
        assert_eq!(qui_a_paye(&carte), "sam@exemple.ca");
        let recu = paiement("ch_z", 1, "succeeded", Value::Null, json!({ "receipt_email": "Sam\u{7}@exemple.ca" }));
        assert_eq!(qui_a_paye(&recu), "Sam@exemple.ca");
        assert_eq!(qui_a_paye(&paiement("ch_w", 1, "succeeded", Value::Null, json!({}))), "");
        // Un remboursement partiel laisse le paiement réussi.
        let partiel = paiement("ch_v", 1_000, "succeeded", Value::Null, json!({ "amount_refunded": 300 }));
        assert_eq!(issue_de(&partiel), Issue::Reussi);
        // Un paiement sans montant ni devise ne se montre pas.
        let sans = json!({ "id": "ch_u", "status": "succeeded" });
        assert!(paiements_de(&[sans]).is_empty());
    }

    #[test]
    fn les_factures_ouvertes() {
        let maintenant = unix(MAINTENANT);
        let o = ouvertes_de(&[], false, "cad", maintenant);
        assert_eq!(o, Ouvertes { nombre: 0, plus: false, du: 0.0, echues: 0 });
        // Envoyée, sans échéance (une facture « à réception ») : pas échue.
        let sans = [facture("in_1", "send_invoice", None, true, 1_000, "cad")];
        assert_eq!(
            ouvertes_de(&sans, true, "cad", maintenant),
            Ouvertes { nombre: 1, plus: true, du: 10.0, echues: 0 }
        );
    }

    #[test]
    fn les_montants_se_divisent_selon_la_devise() {
        assert!(proche(en_unites(12_345.0, "cad"), 123.45));
        assert!(proche(en_unites(500.0, "JPY"), 500.0));
        assert!(proche(en_unites(1_234.0, "kwd"), 1.234));
    }

    #[test]
    fn les_debuts_du_jour_et_du_mois_a_l_heure_du_poste() {
        // Le 27 octobre 2026 à 9 h 12, à l'heure avancée de l'Est : minuit est à 4 h en temps universel.
        assert_eq!(debuts("2026-10-27T09:12:00-04:00"), Some((1_793_073_600, 1_790_827_200)));
        assert_eq!(
            debuts("2027-01-15T15:00:00-05:00"),
            Some((unix("2027-01-15T05:00:00Z"), unix("2027-01-01T05:00:00Z")))
        );
        assert_eq!(debuts("2027-01-15T15:00:00Z"), Some((unix("2027-01-15T00:00:00Z"), unix("2027-01-01T00:00:00Z"))));
        assert_eq!(debuts("illisible"), None);
    }

    #[test]
    fn les_codes_des_droits_refuses() {
        let m =
            "Having the 'rak_invoice_read' permission… also 'rak_customer_read', and 'rak_invoice_read' again; rak_";
        assert_eq!(codes_rak(m), ["rak_invoice_read", "rak_customer_read"]);
        assert!(codes_rak("Forbidden").is_empty());
        // Un droit que la liste d'Eve ne connaît pas se dit par son code.
        let corps = br#"{"error": {"message": "Having the 'rak_balance_transaction_source_read' permission"}}"#;
        assert_eq!(droits_refuses(Route::Transactions, corps), ["rak_balance_transaction_source_read"]);
        assert_eq!(droits_refuses(Route::Paiements, b"pas du JSON"), ["Charges", "Customers"]);
        assert_eq!(
            phrase_des_droits(&["Invoices".into()]).split(" :").next(),
            Some("la clé d'API n'a pas le droit de lire Invoices")
        );
    }

    #[test]
    fn les_montants_restent_une_heure() {
        let t = SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        let (m, _) = lire_avec(reponse_du_compte).0.unwrap();
        let mut p = Partage::default();
        assert_eq!(p.montants(t), None, "sans clé, rien");
        p.cle = true;
        assert_eq!(p.montants(t), None, "avant la première lecture, rien");
        p.dernier = Some((m.clone(), t));
        p.panne = Some("l'API ne répond pas comme il faut (HTTP 503)".into());
        assert_eq!(p.montants(t + VALIDITE), Some(Ok(m.clone())), "une panne en passant laisse les montants une heure");
        let plus_tard = t + VALIDITE + Duration::from_secs(1);
        assert_eq!(p.montants(plus_tard), Some(Err("l'API ne répond pas comme il faut (HTTP 503)".into())));
        p.panne = None;
        assert_eq!(p.montants(plus_tard), Some(Err(PERIMES.into())), "le poste dormait");
        // Une horloge qui recule les garde.
        assert_eq!(p.montants(t - Duration::from_secs(3_600)), Some(Ok(m)));
        p.cle = false;
        assert_eq!(p.montants(t), None, "la clé retirée : plus rien");
    }

    #[test]
    fn une_panne_se_dit_une_fois() {
        let t0 = SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        let mut s = Suivi::nouveau(t0);
        assert!(s.a_lire(t0));
        let d = Duration::from_millis(1_940);
        assert_eq!(s.apres(Ok(7), d, t0), ["Stripe : lu en 1,9 s (7 requêtes GET) ; relu toutes les 10 min."]);
        assert!(!s.a_lire(t0 + Duration::from_secs(9 * 60)));
        assert!(s.a_lire(t0 + PERIODE));
        assert_eq!(s.apres(Ok(8), d, t0 + PERIODE), ["Stripe : lu en 1,9 s (8 requêtes GET)."]);
        // Une panne : une ligne, puis rien jusqu'au retour ; les essais reprennent vite, puis s'espacent.
        let mut t = t0 + PERIODE * 2;
        let (mut dites, mut attentes) = (Vec::new(), Vec::new());
        for _ in 0..6 {
            dites.extend(s.apres(Err("l'API ne répond pas comme il faut (HTTP 502)"), d, t));
            let attente = s.prochaine.duration_since(t).unwrap();
            attentes.push(attente.as_secs() / 60);
            t += attente;
            assert!(s.a_lire(t));
        }
        let dite = "Stripe : l'API ne répond pas comme il faut (HTTP 502) — nouvel essai dans 1 min (une seule ligne \
                    jusqu'au retour).";
        assert_eq!(dites, [dite]);
        assert_eq!(attentes, [1, 2, 4, 8, 10, 10]);
        // Le retour se dit ; la période reprend, et la panne suivante repart à une minute.
        assert_eq!(
            s.apres(Ok(7), d, t),
            ["Stripe : le compte se lit de nouveau.", "Stripe : lu en 1,9 s (7 requêtes GET)."]
        );
        assert_eq!(s.prochaine.duration_since(t).unwrap(), PERIODE);
        assert_eq!(s.apres(Err("x"), d, t).len(), 1);
        assert_eq!(s.prochaine.duration_since(t).unwrap(), REPRISE);
    }

    #[test]
    fn un_changement_de_cle() {
        let t = SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        let une = Some((t, 356));
        // Au premier regard (le démarrage d'Eve), rien ne se dit, avec ou sans clé ; ensuite, ce qui change.
        assert_eq!(changement(None, une), Some(None));
        assert_eq!(changement(None, None), Some(None));
        assert_eq!(changement(Some(une), une), None);
        assert_eq!(changement(Some(None), None), None);
        let posee = Some("Stripe : une clé est posée ; elle se lit tout de suite.");
        assert_eq!(changement(Some(None), une), Some(posee));
        assert_eq!(changement(Some(une), None), Some(Some("Stripe : la clé est retirée ; plus rien ne se lit.")));
        let autre = Some((t + Duration::from_secs(5), 356));
        assert_eq!(changement(Some(une), autre), Some(Some("Stripe : la clé a changé ; elle se lit tout de suite.")));
    }

    #[test]
    fn une_lecture_par_minute_au_plus() {
        let t = Instant::now();
        assert_eq!(reste_avant(None, t), None);
        assert_eq!(reste_avant(Some(t), t), Some(ENTRE_DEUX_LECTURES));
        assert_eq!(reste_avant(Some(t), t + Duration::from_secs(45)), Some(Duration::from_secs(15)));
        assert_eq!(reste_avant(Some(t), t + ENTRE_DEUX_LECTURES), None);
        assert_eq!(reste_avant(Some(t), t + Duration::from_secs(600)), None);
    }

    #[test]
    fn un_texte_de_stripe_se_nettoie() {
        assert_eq!(propre("  Construction\r\nExemple \t inc. "), "Construction Exemple inc.");
        assert_eq!(propre("Sam\u{0}"), "Sam");
        assert_eq!(propre(&"é".repeat(NOM_MAX)), "é".repeat(NOM_MAX));
        assert_eq!(propre(&"é".repeat(NOM_MAX + 1)).chars().count(), NOM_MAX);
    }
}
