//! Le chef d'orchestre : il reçoit tout ce qui arrive — la touche, la fenêtre, les pièces jointes, l'oreille, l'Adjointe,
//! la voix, le veilleur, le battement — par une seule file, dans un seul fil, et décide de la suite. L'état ne se
//! partage pas : il vit ici, et la fenêtre en reçoit le portrait (un JSON) à chaque changement.
//!
//! Ce qu'il tient, en plus de l'échange lui-même :
//! - **un message à la fois** chez l'Adjointe : la suite attend ici, et « arrête » la retire ;
//! - **la parole en flux** : chaque phrase de la réponse se dit dès qu'elle est écrite ([`EnFlux`]), et Eve dit en
//!   quelques mots ce que fait l'Adjointe quand elle cherche (la base, les courriels, le calendrier) ;
//! - **le préchauffage** : au démarrage, un premier tour silencieux met le contexte de l'Adjointe en cache, avec la
//!   mémoire des derniers échanges ([`memoire`]) ;
//! - **les alertes** : un courriel suspect, une facture, un rendez-vous qui approche — une cloche, puis une phrase ;
//! - **l'accord** : chaque écriture attend « je confirme » (ou un clic sur la demande affichée, par son numéro) ;
//! - **la caméra** : « Eve, scanne cette facture » l'ouvre dans la fenêtre, « photo » prend une page, « classe-la »
//!   range le PDF dans `Scan\` et le confie à l'Adjointe (règle 7 du `CLAUDE.md`) ;
//! - **l'historique** : chaque échange est gardé sur le poste ([`historique`]) ; la fenêtre le relit et en reprend un ;
//! - **les profils** ([`profils`]) : le profil choisi dans `reglages.json`, où Claude Code repart entre deux demandes,
//!   chacun avec sa conversation ;
//! - **Constructo AI** (le privilège `constructo`) : la connexion de cette Eve à l'ERP ([`connexion`]), son
//!   renouvellement, et son branchement dans Claude Code, au lancement et à chaque jeton neuf ;
//! - **la mise à jour** ([`mise_a_jour`]) : une relance demandée attend la fin du tour, puis le gardien prend le relais
//!   et Eve se ferme ; la marque de santé, cinq secondes après le démarrage, la fenêtre ouverte.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};

use eve::annuler::{self, Annulations};
use eve::garde::{self, Decision, Espace};
use eve::mesures::{self, Etape, Temps};
use eve::parole::{self, EnFlux, GestePhoto, Transcription};
use eve::protocole::{self, Flux, Recu};
use serde_json::{Value, json};

use crate::cerveau::{self, Cerveau, Lancement};
use crate::connexion::{self, EnCours, Etat as Connexion, Lien, Refus, Retour};
use crate::ecran::{self, Ecran};
use crate::fenetre;
use crate::historique::{self, Entree};
use crate::journal;
use crate::memoire::{self, Echange};
use crate::micro::{self, Micro};
use crate::mise_a_jour::{self, Empeche, Relance};
use crate::oreille::Demande;
use crate::profils::{self, Laisse, Laisses};
use crate::rappels::{self, Rappels};
use crate::reglages::{Profil, Reglages, dossier_local, nom_affiche};
use crate::sons::{self, Son};
use crate::touche;
use crate::veilleur::{Jour, Nouvelle, RendezVous, Sorte};
use crate::voix::{Voix, VoixDispo};

/// Tout ce qui arrive au chef.
pub enum Evt {
    /// La touche à tenir : enfoncée (`true`) ou relâchée.
    Touche(bool),
    /// Un geste de la fenêtre.
    Ui {
        action: String,
        valeur: Value,
    },
    /// Une pièce jointe déposée dans la fenêtre.
    Piece {
        nom: String,
        chemin: PathBuf,
        taille: u64,
    },
    /// Une numérisation de la caméra, réunie en PDF dans le dossier local ; `but` : « classer » ou « joindre » ;
    /// `sorte` : « document », « couleur » ou « photo ».
    Scan {
        nom: String,
        chemin: PathBuf,
        pages: usize,
        but: String,
        sorte: String,
        taille: u64,
    },
    /// L'oreille est prête (ou ne l'est pas).
    Oreille(Result<String, String>),
    /// Une phrase transcrite ; `continue_` : entendue par l'écoute continue (pendant une demande d'accord, ou après
    /// « Oui ? »), pas à la touche ; `depuis` : la fin de la parole.
    Transcrit {
        texte: String,
        secondes_audio: f32,
        secondes_calcul: f32,
        continue_: bool,
        depuis: Instant,
    },
    /// L'écoute continue a entendu « Eve » : la demande se transcrit.
    EveilEntendu,
    /// La demande qui suivait « Eve » (vide : « Eve » tout seul) ; `depuis` : la fin de la phrase.
    Eveil {
        demande: String,
        depuis: Instant,
    },
    /// Un message de l'Adjointe (Claude Code).
    Claude {
        generation: u64,
        message: Value,
    },
    /// L'Adjointe s'est arrêtée.
    ClaudeFini {
        generation: u64,
    },
    /// Les voix du poste, et celle qui parle.
    Voix {
        liste: Vec<VoixDispo>,
        choisie: String,
    },
    /// Le premier son d'une parole s'entend.
    VoixEntendue(u64),
    /// La voix a fini (ou s'est tue).
    VoixFinie(u64),
    VoixErreur(String),
    /// Le battement (toutes les 100 ms).
    Tic,
    /// Le jeton d'Adjointe, lu au démarrage dans un fil (`None` : aucun jeton valable sur ce poste).
    Jeton(Option<String>),
    /// Une nouvelle de la veille des courriels (une ligne de son journal).
    Nouvelle(Nouvelle),
    /// Un rendez-vous du calendrier approche.
    RendezVous(RendezVous),
    /// Les rendez-vous du jour (relus toutes les deux minutes), pour « quoi de neuf ».
    Agenda(Vec<RendezVous>),
    /// Les quinze jours du calendrier en ligne, relus à l'heure `lu` : joints à une question de calendrier.
    Calendrier {
        lu: String,
        jours: Vec<Jour>,
    },
    /// Le retour de la page de connexion de Constructo AI (OAuth), reçu par la vitrine.
    Retour(Retour),
    /// La connexion à Constructo AI a avancé, dans un fil qui parle au serveur ; `tour` : le tour de connexion qui l'a
    /// lancé (une déconnexion en ouvre un autre, et la réponse d'un tour abandonné est écartée).
    Connexion {
        tour: u64,
        etape: EtapeConnexion,
    },
}

/// Une étape de la connexion à Constructo AI, rapportée par le fil qui parle au serveur.
pub enum EtapeConnexion {
    /// Eve est enregistrée : la page de connexion à ouvrir dans le navigateur.
    Page { etat: Connexion, en_cours: EnCours, url: String },
    /// Des jetons neufs (la connexion, ou un renouvellement), déjà écrits sur le disque.
    Jetons(Connexion),
    /// La connexion n'a pas abouti (pour l'écran).
    Echec(String),
    /// Le renouvellement a échoué.
    Renouvellement(Refus),
}

/// La demande `mcp_set_servers` en vol, qui branche Constructo AI dans Claude Code.
struct Branchement {
    id: String,
    depuis: Instant,
    /// Les demandes de la personne l'attendent encore : sans elle, la première partirait sans les outils de l'ERP.
    retient: bool,
}

/// Après un renouvellement manqué pour une raison passagère (le réseau, le serveur qui se réveille), le suivant attend
/// ceci, puis le double, jusqu'à un quart d'heure.
const RENOUVELER_APRES: Duration = Duration::from_secs(120);
/// Le branchement de Constructo AI ne retient jamais les demandes de la personne plus longtemps.
const BRANCHEMENT_MAX: Duration = Duration::from_secs(45);
/// Le compte rendu d'un agent qui a fini retient un changement de Claude (profil, cerveau, mise à jour) ce temps au
/// plus, compté depuis la fin de l'agent ou du dernier tour : vu le 24 octobre 2026, il vient une seconde après.
const COMPTE_RENDU_MAX: Duration = Duration::from_secs(20);
/// Des réglages de Claude changés à moins de ce temps l'un de l'autre (le modèle, puis l'effort) ne le relancent
/// qu'une fois.
const REGROUPER: Duration = Duration::from_secs(2);
/// Ce que la fenêtre dit pendant que la page de connexion de Constructo AI attend la personne.
const PAGE_DE_CONNEXION_OUVERTE: &str = "La page de connexion de Constructo AI est ouverte dans ton navigateur : entre \
                                         le courriel et le mot de passe de ton entreprise, puis reviens ici.";

/// Le temps laissé pour dire « je confirme » ; au-delà, l'écriture est refusée.
const ACCORD_DELAI: Duration = Duration::from_secs(180);
/// Une réponse d'accord arrivée si tôt après un changement de demande ne vaut pas pour la nouvelle : Eve la redemande.
const ACCORD_GARDE: Duration = Duration::from_millis(1500);
/// Après ce temps de travail sans un mot, Eve dit « Je m'en occupe ».
const ANNONCE_APRES: Duration = Duration::from_secs(5);
/// L'écoute à la touche : jamais plus longue (une touche relâchée hors de la fenêtre ne se voit pas toujours).
const ECOUTE_MAX: Duration = Duration::from_secs(45);
/// La touche tenue au-delà coupe la voix ; un raccourci clavier (Ctrl+C) ne la coupe pas.
const COUPE_APRES: Duration = Duration::from_millis(350);
/// Après « Oui ? », le temps de commencer sa demande.
const SUITE_DELAI: Duration = Duration::from_secs(8);
/// La conversation du jour ne reprend après un redémarrage que si son dernier tour date de moins que cela : au-delà,
/// le cache de Claude a refroidi, et reprendre une longue conversation coûterait plus qu'en ouvrir une neuve (la
/// mémoire courte fait le pont).
const REPRISE_MAX: i64 = 3600;
/// Une conversation neuve chaque jour : passé cette heure, au premier moment libre après ce calme.
const NOUVELLE_JOURNEE_A: u32 = 4;
const CALME_AVANT_LA_NOUVELLE: Duration = Duration::from_secs(30 * 60);

/// Claude Code qui n'a pas répondu à `initialize` en ce temps-là est relancé (le serveur de Constructo AI qui dort
/// sur Render prend jusqu'à une minute à s'éveiller).
const INIT_DELAI: Duration = Duration::from_secs(120);
/// Ce qu'Eve a dit dans ce temps-là peut revenir par le micro : le garde-fou de l'oreille le reconnaît.
const DIT_RECEMMENT: Duration = Duration::from_secs(30);
/// L'état ne se publie pas plus souvent (le texte en cours d'écriture change à chaque mot).
const PUBLICATION_MIN: Duration = Duration::from_millis(80);
/// Le temps, au plus, que le chef publie l'émotion du dernier passage dit, une fois Eve tue : le visage la quitte bien
/// avant, le temps qu'une personne la garderait (sa tenue, `natif\portrait\emotions.rs`).
const EMOTION_DUREE: Duration = Duration::from_secs(90);
/// Le contexte de l'Adjointe reste une heure en cache chez Anthropic (cerveau.rs) : passé ce temps sans un tour, la
/// question suivante repartirait à froid (13 à 24 s avant le premier mot, mesuré le 2026-10-22, au lieu de 5 à 6 s). Un
/// tour silencieux le garde au chaud, aux heures de bureau.
const AU_CHAUD_APRES: Duration = Duration::from_secs(50 * 60);
const FIL_MAX: usize = 80;
/// Les interrupteurs des réglages de la fenêtre (le geste `reglage`) : seuls ceux-là s'y changent, jamais un privilège.
const INTERRUPTEURS: [&str; 9] = [
    "eveil", "interruption", "sourdine", "alertes.suspects", "alertes.factures", "alertes.pieces",
    "alertes.rendez_vous", "devant", "historique",
];

/// Comment Eve nomme, à voix haute et dans son fil, celle qui travaille derrière elle : l'Adjointe de la trousse, ou
/// Claude là où le dossier porte sa propre consigne (une Eve qui n'est pas celle d'Entrepreneur Général).
struct Nommee {
    /// « l'Adjointe », « Claude ».
    le: &'static str,
    /// En tête de phrase.
    le_maj: &'static str,
    /// « de l'Adjointe », « de Claude ».
    de: &'static str,
    /// L'accord du participe : « e » au féminin.
    e: &'static str,
    /// Le pronom complément : « la », « le ».
    la: &'static str,
}

const ADJOINTE: Nommee = Nommee { le: "l'Adjointe", le_maj: "L'Adjointe", de: "de l'Adjointe", e: "e", la: "la" };
const CLAUDE: Nommee = Nommee { le: "Claude", le_maj: "Claude", de: "de Claude", e: "", la: "le" };

#[derive(Clone, Copy, PartialEq, Eq)]
enum Sorte2 {
    Crochet,
    Permission,
}

struct Attente {
    /// Le numéro affiché : un clic ne vaut que pour la demande qu'on voyait.
    numero: u64,
    id: String,
    sorte: Sorte2,
    quoi: String,
    detail: String,
    entree: Value,
    depuis: Instant,
}

struct Ligne {
    de: &'static str,
    texte: String,
    heure: String,
    pieces: Vec<String>,
    /// Un échange repris d'une séance précédente.
    ancien: bool,
    /// Sous une réponse de Claude : ce que son tour a pris (« 12 s · 0,04 $ · Sonnet 5.5, moyen ») ; vide ailleurs.
    tour: String,
}

struct Piece {
    id: String,
    nom: String,
    chemin: PathBuf,
    taille: u64,
}

/// Un message pour l'Adjointe ; `demande` : ce que la personne a dit (aucune pour un tour silencieux).
struct Message {
    texte: String,
    demande: Option<String>,
    /// Les pièces jointes à la demande (leur nom), pour l'historique.
    pieces: Vec<String>,
    /// Un tour pour garder le contexte au chaud.
    au_chaud: bool,
    /// La demande est venue de la voix : ses suppressions se confirment ([`garde::suppression`]).
    a_la_voix: bool,
    /// Les temps de la demande (aucun pour un tour silencieux).
    chrono: Option<Chrono>,
}

impl Message {
    /// Un tour silencieux : le préchauffage, ou un tour qui garde le contexte au chaud.
    fn silencieux(texte: impl Into<String>, au_chaud: bool) -> Message {
        Message { texte: texte.into(), demande: None, pieces: Vec::new(), au_chaud, a_la_voix: false, chrono: None }
    }
}

/// Les temps d'une demande, de la fin de la parole de la personne au premier son de la réponse : notés au journal
/// (« mesures du tour ») quand le tour est fini et sa première phrase entendue — ou quand elle ne peut plus l'être.
/// `eve --mesures` en tire les médianes ([`eve::mesures`]).
struct Chrono {
    /// La fin de la parole (la touche relâchée, la phrase finie) ; une demande écrite : son arrivée.
    depuis: Instant,
    /// Le texte de la demande est prêt (dite : transcrite).
    prete: Instant,
    /// Dite : elle a passé par l'oreille.
    dite: bool,
    envoyee: Option<Instant>,
    premier_mot: Option<Instant>,
    premiere_phrase: Option<Instant>,
    /// Le numéro de parole de la première phrase, que la voix dira entendue (aucun en sourdine, ou la voix coupée).
    parole_id: Option<u64>,
    entendue: Option<Instant>,
}

impl Chrono {
    /// `fin_de_parole` : celle d'une demande dite ; aucune pour une demande écrite.
    fn nouveau(fin_de_parole: Option<Instant>) -> Chrono {
        let maintenant = Instant::now();
        Chrono {
            depuis: fin_de_parole.unwrap_or(maintenant),
            prete: maintenant,
            dite: fin_de_parole.is_some(),
            envoyee: None,
            premier_mot: None,
            premiere_phrase: None,
            parole_id: None,
            entendue: None,
        }
    }

    /// La première phrase est donnée à la voix, mais pas encore entendue.
    fn attend_la_voix(&self) -> bool {
        self.parole_id.is_some() && self.entendue.is_none()
    }

    /// Les étapes, chacune depuis la précédente ; la première parole, depuis la fin de la parole.
    fn temps(&self) -> Temps {
        let entre = |a: Instant, b: Instant| b.saturating_duration_since(a).as_secs_f64();
        let mut t = Temps::default();
        if self.dite {
            t.poser(Etape::Oreille, entre(self.depuis, self.prete));
        }
        if let Some(e) = self.envoyee {
            t.poser(Etape::Attente, entre(self.prete, e));
            if let Some(m) = self.premier_mot {
                t.poser(Etape::PremierMot, entre(e, m));
            }
            if let Some(p) = self.premiere_phrase {
                t.poser(Etape::PremierePhrase, entre(self.premier_mot.unwrap_or(e), p));
            }
        }
        if let (Some(p), Some(h)) = (self.premiere_phrase, self.entendue) {
            t.poser(Etape::Voix, entre(p, h));
            t.poser(Etape::PremiereParole, entre(self.depuis, h));
        }
        t
    }
}

/// Les émotions d'une réponse, passage par passage (« [joie] Bonne nouvelle ! [serieuse] Par contre… ») : le visage prend
/// celle d'un passage quand il se dit, pas quand il s'écrit — la voix a souvent une phrase de retard sur Claude. Le ton
/// du passage qui s'écrit, et le premier de la réponse ; celui de la dernière phrase donnée à la voix, et son numéro de
/// parole ; les passages dont l'émotion attend que leur phrase se dise.
#[derive(Default)]
struct Passages {
    ton: Option<parole::Emotion>,
    premier: Option<parole::Emotion>,
    ton_dit: Option<parole::Emotion>,
    derniere: Option<u64>,
    attente: VecDeque<Passage>,
}

/// Un passage qui attend de se dire : son émotion se montre quand la phrase d'avant (`avant`, son numéro de parole) a
/// fini — dans le silence qui sépare les deux phrases, comme on prend son air avant de parler —, ou au plus tard quand la
/// sienne (`parole`) s'entend.
struct Passage {
    avant: u64,
    parole: u64,
    emotion: parole::Emotion,
}

impl Passages {
    /// Le ton change : le passage qui commence a cette émotion.
    fn changer(&mut self, e: parole::Emotion) {
        self.ton = Some(e);
        self.premier.get_or_insert(e);
    }

    /// Une phrase de la réponse est donnée à la voix (`parole` : son numéro de parole ; aucun : elle ne se dit pas, en
    /// sourdine) : l'émotion à montrer tout de suite, si son passage en change — la première phrase dite (le visage la
    /// prend pendant que le son se prépare, un instant avant qu'il s'entende), ou une phrase qui ne se dit pas (elle
    /// s'écrit : on la lit) ; celle d'une phrase qui en suit une autre attend son tour ([`Passages::voix`]).
    fn phrase(&mut self, parole: Option<u64>) -> Option<parole::Emotion> {
        let nouvelle = self.ton.filter(|&e| self.ton_dit != Some(e));
        let montrer = match (nouvelle, parole, self.derniere) {
            (Some(emotion), Some(parole), Some(avant)) => {
                self.attente.push_back(Passage { avant, parole, emotion });
                None
            }
            (Some(e), _, _) => {
                self.attente.clear();
                Some(e)
            }
            (None, _, _) => None,
        };
        if nouvelle.is_some() {
            self.ton_dit = nouvelle;
        }
        if parole.is_some() {
            self.derniere = parole;
        }
        montrer
    }

    /// La voix avance — le premier son de la parole `id` s'entend (`entendue`), ou elle a fini (dite, ou en échec) :
    /// l'émotion du dernier passage qu'elle atteint, à montrer.
    fn voix(&mut self, id: u64, entendue: bool) -> Option<parole::Emotion> {
        let mut e = None;
        while let Some(p) = self.attente.front()
            && (p.parole <= id || (!entendue && p.avant <= id))
        {
            e = Some(p.emotion);
            self.attente.pop_front();
        }
        e
    }

    /// La voix est coupée : les passages qui attendaient ne se diront pas ; la phrase suivante, s'il en vient une, montre
    /// le sien tout de suite.
    fn couper(&mut self) {
        self.attente.clear();
        self.derniere = None;
    }

    /// Aucun passage n'attend.
    fn vide(&self) -> bool {
        self.attente.is_empty()
    }

    /// L'émotion du premier passage de la réponse.
    fn premier(&self) -> Option<parole::Emotion> {
        self.premier
    }
}

pub struct Chef {
    reglages: Reglages,
    dossier: Option<PathBuf>,
    claude: Option<PathBuf>,
    ecran: Ecran,
    voix: Voix,
    oreille: Sender<Demande>,
    rapport: Sender<Evt>,
    avec_fenetre: bool,
    // Le micro et l'oreille
    micro: Option<Micro>,
    micro_nom: String,
    micro_essai: Instant,
    micro_delai: Duration,
    micro_signale: bool,
    ecoute: bool,
    debut_ecoute: Option<Instant>,
    transcriptions: u32,
    /// Après « Oui ? » : la phrase suivante est la demande, sans « Eve » devant (jusqu'à cet instant).
    ecoute_suite: Option<Instant>,
    /// Le « Oui ? » en train de se dire (son numéro de parole) : l'écoute de la suite commence quand il est dit.
    oui: Option<u64>,
    /// La fin de la dernière parole d'Eve : le micro reste en sourdine un instant après (l'écho de la pièce).
    fin_parole: Option<Instant>,
    /// Un petit son en train de jouer : sourdine jusque-là.
    sourdine_jusqua: Option<Instant>,
    oreille_etat: (&'static str, String),
    // L'Adjointe
    cerveau: Option<Cerveau>,
    generation: u64,
    id_init: String,
    init_ok: bool,
    /// Le jeton d'Adjointe : `None` tant qu'il n'est pas lu, `Some(None)` s'il n'y en a pas.
    jeton: Option<Option<String>>,
    session: Option<String>,
    /// La prochaine ouverture reprend la session (après un plantage).
    reprendre: bool,
    lance_en_reprise: bool,
    /// Le préchauffage a commencé avant que la base soit branchée : ses outils et ses consignes sont arrivés en cours
    /// de route, et le contexte mis en cache n'est plus le bon (mesuré le 2026-10-22). Un second tour silencieux suit.
    rechauffer: bool,
    rechauffages: u8,
    en_attente: VecDeque<Message>,
    tours: u32,
    /// Le tour en cours est silencieux (le préchauffage, ou un tour qui garde le contexte au chaud) : sa réponse ne se
    /// dit ni ne s'affiche.
    prechauffage: bool,
    /// … et c'est un tour pour garder le contexte au chaud : Eve reste « prête » à l'écran.
    au_chaud: bool,
    /// Le dernier tour fini (ou commencé) : le contexte est au chaud depuis.
    dernier_tour: Instant,
    interrompu: bool,
    demande_du_tour: Option<String>,
    /// La demande du tour est venue de la voix.
    tour_a_la_voix: bool,
    /// Le numéro du tour en cours : les copies d'« annule la dernière modification » vont par tour.
    tour_numero: u64,
    /// Les copies des fichiers d'avant leur écriture ([`Annulations`]).
    annulations: Annulations,
    /// Les rappels et les minuteries (`rappels.json` du dossier local), et la dernière fois qu'on les a regardés.
    rappels: Rappels,
    rappels_vus: Option<Instant>,
    /// La panne de Claude vue dans le tour (sa sorte : « rate_limit », « overloaded », « reseau »…), dite à sa fin ; un
    /// nouvel essai de l'API déjà dit dans le tour.
    panne: String,
    reessai_dit: bool,
    /// La limite d'utilisation du compte : l'instant (Unix) où elle se rouvre, quand elle est atteinte ; et la dernière
    /// limite dite (une seule fois chacune).
    limite: Option<i64>,
    limite_dite: Option<(String, Option<i64>)>,
    /// Depuis quand Claude Code attend sa réponse à `initialize` : au-delà d'[`INIT_DELAI`], il est relancé.
    init_attendue: Option<Instant>,
    /// Le jour où la conversation de Claude a commencé (« 2026-10-25 ») : un autre jour, elle se renouvelle.
    seance_du: Option<String>,
    /// Ce qu'Eve a fait de son côté (une annulation) : dit à l'Adjointe avec la demande suivante.
    note_pour_l_adjointe: Option<String>,
    /// Les temps de la demande en cours ; ceux d'une demande finie dont la première phrase n'est pas encore entendue.
    chrono: Option<Chrono>,
    chrono_a_entendre: Option<Chrono>,
    /// Le cumul que Claude Code a rendu à la fin du tour d'avant (le coût de sa séance) : le coût d'un tour en est la
    /// différence. Il repart à chaque lancement.
    cumul: Option<f64>,
    /// La dernière réponse, en pages pour la voix (`parole::pages_pour_la_voix`), et la prochaine page à dire : la
    /// première s'est dite avec la réponse ; « la suite » dit les autres, « répète » la dernière dite.
    pages: Vec<String>,
    page: usize,
    /// Les phrases qu'Eve vient de dire (30 s) : le garde-fou de l'oreille y reconnaît sa propre voix revenue par le
    /// micro.
    dit_recemment: VecDeque<(Instant, String)>,
    /// La dernière phrase mal entendue (son numéro, son texte), publiée : la fenêtre la met dans la saisie, à corriger.
    a_corriger: Option<(u64, String)>,
    pieces_du_tour: Vec<String>,
    attentes: VecDeque<Attente>,
    numero_accord: u64,
    tete_depuis: Instant,
    /// Les outils que la garde a vus passer (leur identifiant), et leur signature quand l'identifiant manque.
    vus: HashSet<String>,
    signatures: HashMap<String, u32>,
    /// Les outils annoncés par l'Adjointe, en attente de leur résultat.
    en_route: HashMap<String, (String, Value)>,
    base: (&'static str, String),
    /// L'espace d'actions de Claude, fixé au démarrage : ses privilèges ([`Reglages::espace`]). Claude Code n'en reçoit
    /// pas d'autres, et la garde refuse ce qui en sort.
    espace: Espace,
    /// Les outils offerts par Claude Code ont été vus (une fois par lancement) : un outil hors de l'espace se signale.
    outils_vus: bool,
    // Constructo AI (le privilège `constructo`) : où se relier, ce qui est gardé, la connexion en cours dans le
    // navigateur, le fil qui parle au serveur (un seul à la fois) ; le tour de connexion — une déconnexion l'avance, et
    // ce qu'un fil d'un tour précédent rapporte est écarté (un renouvellement parti avant « Se déconnecter » ne ressuscite
    // pas la connexion) ; le prochain essai de renouvellement ; le branchement en vol ; le serveur branché dans la séance
    // de Claude Code ; la dernière panne, pour la fenêtre.
    lien: Lien,
    connexion: Connexion,
    connexion_en_cours: Option<EnCours>,
    connexion_occupee: bool,
    connexion_tour: u64,
    renouveler_a: Option<Instant>,
    renouveler_delai: Duration,
    branchement: Option<Branchement>,
    constructo_branche: bool,
    constructo_erreur: Option<String>,
    /// Le dossier porte sa propre consigne (`EVE_CONSIGNE.md`, un poste d'architecture) : ce n'est pas l'Adjointe de la
    /// trousse — ni la base d'Entrepreneur Général, ni le `Scan\` où classer une numérisation.
    propre: bool,
    /// Le profil où travaille Claude Code ([`profils`]) ; `None` : celui d'office, le dossier d'Eve elle-même
    /// (`dossier`).
    profil: Option<Profil>,
    /// Ce que les profils quittés ont laissé : leur séance et leur conversation, reprises à leur retour.
    laisses: Laisses,
    /// Le profil, le modèle ou l'effort de Claude ont changé dans reglages.json : Claude Code repart avec eux dès
    /// qu'aucun tour ne court et qu'aucun agent ne travaille ([`Self::suivre_claude`]) ; les demandes l'attendent, pour
    /// partir dans le bon dossier, avec le bon cerveau.
    claude_a_suivre: bool,
    /// Le modèle et l'effort de son lancement : s'ils changent dans les réglages, Claude Code repart.
    lance_avec: (String, String),
    /// Les agents que Claude fait travailler en arrière-plan (leur description, `background_tasks_changed`) : la
    /// séance ne s'arrête pas sous eux — ni pour un autre profil, ni pour un autre cerveau, ni pour une mise à jour.
    agents: Vec<String>,
    /// Un agent a fini et Claude n'en a pas encore rendu compte (depuis quand) : son compte rendu vient dans un tour
    /// qu'il ouvre de lui-même, une seconde plus tard ; la séance ne s'arrête pas avant ([`COMPTE_RENDU_MAX`] au plus).
    compte_rendu_attendu: Option<Instant>,
    /// Quand les réglages de Claude ont changé la dernière fois : deux clics de suite (le modèle, puis l'effort) ne le
    /// relancent qu'une fois ([`REGROUPER`]).
    claude_change_a: Instant,
    erreur: Option<String>,
    relances: Vec<Instant>,
    // La voix
    voix_liste: Vec<VoixDispo>,
    voix_choisie: String,
    parle: bool,
    parole_id: u64,
    flux: EnFlux,
    /// Les émotions de la réponse qui arrive (« [triste] » en tête, « [serieuse] » où le ton change), ôtées du texte
    /// ([`parole::Tons`]) ; ses passages, chacun avec la sienne, que le visage prend quand il se dit ([`Passages`]).
    tons: parole::Tons,
    passages: Passages,
    /// L'émotion que montre le visage d'Eve — celle du passage qu'elle dit —, et depuis quand.
    emotion: Option<(parole::Emotion, Instant)>,
    dit_dans_le_tour: bool,
    commentaires: HashSet<&'static str>,
    dernier_dit: Option<Instant>,
    erreur_voix: Option<Instant>,
    // Le tour
    debut_tour: Option<Instant>,
    dernier_signe: Instant,
    annonce: bool,
    /// Ce qu'Eve a déjà dit d'un travail qui s'allonge (0 : rien ; 1 : à une minute ; 2 : à deux minutes et demie).
    rassure: u8,
    // L'écran
    fil: Vec<Ligne>,
    fil_rev: u64,
    en_cours: String,
    activite: VecDeque<String>,
    /// Les derniers outils de Claude, leur début et leur fin (`noter_outil`) : le réseau des actions de la fenêtre
    /// native les montre en direct ; `outils_n` les numérote, pour qu'elle sache lesquels sont nouveaux.
    outils: VecDeque<Value>,
    outils_n: u64,
    pieces: Vec<Piece>,
    compteur_pieces: u64,
    demarrage_windows: bool,
    // Le veilleur et la mémoire
    jour: String,
    nouvelles: Vec<Nouvelle>,
    agenda: Vec<RendezVous>,
    calendrier: Vec<Jour>,
    calendrier_lu: String,
    dernier_point: Option<String>,
    alertes: VecDeque<String>,
    alerte_a_dire: Option<(Instant, String)>,
    memoire: Vec<Echange>,
    memoire_a_donner: bool,
    // L'historique : la conversation en cours (l'heure où elle a commencé), et une conversation reprise à donner à
    // l'Adjointe avec la demande suivante.
    conversation: String,
    reprise: Option<String>,
    // La caméra : ouverte dans la fenêtre ; le dernier ordre donné de la voix (son numéro, pour qu'il ne s'exécute
    // qu'une fois) ; ce que la personne a dit en l'ouvrant et en classant.
    camera: bool,
    camera_ordre: (u64, &'static str),
    intentions_scan: Vec<String>,
    // La fenêtre
    sans_client: Option<Instant>,
    a_eu_client: bool,
    fenetre_fermee: bool,
    quitter: bool,
    dernier_publie: String,
    publie_a: Instant,
    // La mise à jour ([`mise_a_jour`]) : la relance demandée, qui attend la fin du tour, et quand Eve l'a dite ; le
    // démarrage, la marque de santé posée (cinq secondes après, la fenêtre ouverte), et ce qu'Eve dira alors — sa
    // nouvelle version ; la version précédente, que les réglages offrent, et quand elle a été lue (le gardien n'inscrit
    // une version qu'une fois sa marque posée : elle se relit).
    relance: Option<Relance>,
    relance_dite: Option<Instant>,
    demarre: Instant,
    marque_posee: bool,
    a_la_sante: Option<(String, String)>,
    precedente: Option<String>,
    precedente_lue: Instant,
}

impl Chef {
    pub fn nouveau(
        reglages: Reglages,
        ecran: Ecran,
        voix: Voix,
        oreille: Sender<Demande>,
        rapport: Sender<Evt>,
        avec_fenetre: bool,
    ) -> Chef {
        let dossier = reglages.dossier_adjointe();
        let propre = dossier.as_ref().is_some_and(|d| cerveau::consigne_du_dossier(d, "").is_some());
        // Le profil choisi : Claude Code part tout de suite dans son dossier ; s'il ne peut pas servir, dans celui
        // d'Eve.
        let (profil, profil_refuse) = match profils::trouver(&reglages, nom_affiche(), &reglages.profil) {
            Ok(p) => (p, None),
            Err(raison) => (None, Some(raison)),
        };
        // Sans dossier, Claude ne se lance pas ; l'espace, lui, reste celui d'un spécialiste.
        let espace = reglages.espace(reglages.specialiste());
        let lien = Lien::du_poste();
        let connexion = if espace.constructo { lien.charger() } else { Connexion::default() };
        let claude = cerveau::trouver_claude();
        let micro = match Micro::ouvrir() {
            Ok(m) => {
                journal::ecrire(&format!("micro : {}", m.nom));
                m.ecoute_continue(reglages.eveil);
                Some(m)
            }
            Err(e) => {
                journal::ecrire(&format!("micro : {e}"));
                None
            }
        };
        let erreur = match (&dossier, &claude) {
            (None, _) => Some(dossier_introuvable()),
            (_, None) => Some("Claude Code est introuvable sur ce poste.".into()),
            _ => None,
        };
        let memoire = if reglages.memoire { memoire::charger(&reglages.qui) } else { Vec::new() };
        // Ce que le gardien d'une relance a laissé pour cette Eve : la conversation à reprendre, ce qu'elle dira.
        let depart = mise_a_jour::au_demarrage();
        let maintenant = Instant::now();
        let mut chef = Chef {
            micro_nom: micro.as_ref().map(|m| m.nom.clone()).unwrap_or_default(),
            reglages,
            dossier,
            claude,
            ecran,
            voix,
            oreille,
            rapport,
            avec_fenetre,
            micro,
            micro_essai: maintenant,
            micro_delai: Duration::from_secs(5),
            micro_signale: false,
            ecoute: false,
            debut_ecoute: None,
            transcriptions: 0,
            ecoute_suite: None,
            oui: None,
            fin_parole: None,
            sourdine_jusqua: None,
            oreille_etat: ("chargement", "L'oreille se charge".into()),
            cerveau: None,
            generation: 1,
            id_init: String::new(),
            init_ok: false,
            jeton: None,
            session: None,
            reprendre: false,
            lance_en_reprise: false,
            rechauffer: false,
            rechauffages: 0,
            en_attente: VecDeque::new(),
            tours: 0,
            prechauffage: false,
            au_chaud: false,
            dernier_tour: maintenant,
            interrompu: false,
            demande_du_tour: None,
            tour_a_la_voix: false,
            tour_numero: 0,
            annulations: Annulations::nouvelles(dossier_local().join("annuler")),
            rappels: Rappels::ouvrir(&dossier_local()),
            rappels_vus: None,
            panne: String::new(),
            reessai_dit: false,
            limite: None,
            limite_dite: None,
            init_attendue: None,
            seance_du: None,
            note_pour_l_adjointe: None,
            chrono: None,
            chrono_a_entendre: None,
            cumul: None,
            pages: Vec::new(),
            page: 0,
            dit_recemment: VecDeque::new(),
            a_corriger: None,
            pieces_du_tour: Vec::new(),
            attentes: VecDeque::new(),
            numero_accord: 0,
            tete_depuis: maintenant,
            vus: HashSet::new(),
            signatures: HashMap::new(),
            en_route: HashMap::new(),
            base: ("inconnue", "lecture du jeton d'Adjointe…".into()),
            espace,
            outils_vus: false,
            lien,
            connexion,
            connexion_en_cours: None,
            connexion_occupee: false,
            connexion_tour: 0,
            renouveler_a: None,
            renouveler_delai: RENOUVELER_APRES,
            branchement: None,
            constructo_branche: false,
            constructo_erreur: None,
            propre,
            profil,
            laisses: Laisses::default(),
            claude_a_suivre: false,
            lance_avec: (String::new(), String::new()),
            agents: Vec::new(),
            compte_rendu_attendu: None,
            claude_change_a: maintenant,
            erreur,
            relances: Vec::new(),
            voix_liste: Vec::new(),
            voix_choisie: String::new(),
            parle: false,
            parole_id: 0,
            flux: EnFlux::default(),
            tons: parole::Tons::default(),
            passages: Passages::default(),
            emotion: None,
            dit_dans_le_tour: false,
            commentaires: HashSet::new(),
            dernier_dit: None,
            erreur_voix: None,
            debut_tour: None,
            dernier_signe: maintenant,
            annonce: false,
            rassure: 0,
            fil: Vec::new(),
            fil_rev: 0,
            en_cours: String::new(),
            activite: VecDeque::new(),
            outils: VecDeque::new(),
            outils_n: 0,
            pieces: Vec::new(),
            compteur_pieces: 0,
            demarrage_windows: fenetre::demarrage_windows_actif(),
            jour: journal::date(),
            nouvelles: Vec::new(),
            agenda: Vec::new(),
            calendrier: Vec::new(),
            calendrier_lu: String::new(),
            dernier_point: None,
            alertes: VecDeque::new(),
            alerte_a_dire: None,
            memoire_a_donner: !memoire.is_empty(),
            memoire,
            conversation: journal::horodatage(),
            reprise: None,
            camera: false,
            camera_ordre: (0, ""),
            intentions_scan: Vec::new(),
            sans_client: None,
            a_eu_client: false,
            fenetre_fermee: false,
            quitter: false,
            dernier_publie: String::new(),
            publie_a: maintenant,
            relance: None,
            relance_dite: None,
            demarre: maintenant,
            marque_posee: false,
            a_la_sante: depart.a_la_sante,
            precedente: mise_a_jour::precedente_offerte(),
            precedente_lue: maintenant,
        };
        chef.montrer_la_memoire();
        if let Some(p) = &chef.profil {
            let m = format!("Profil : {} ({}).", p.nom, p.dossier.display());
            journal::ecrire(&m);
            chef.systeme(&m);
        }
        if let Some(raison) = profil_refuse {
            let m = format!("{raison} Eve démarre dans son profil d'office, {}.", nom_affiche());
            journal::ecrire(&m);
            chef.systeme(&m);
        }
        // Après une relance, la conversation de Claude reprend telle quelle (`--resume`, sans préchauffage) : sa
        // mémoire des derniers échanges n'a pas à lui être redonnée.
        if let Some(session) = depart.session {
            chef.session = Some(session);
            chef.reprendre = true;
            chef.memoire_a_donner = false;
        } else if let Some(session) = seance_a_reprendre(chef.profil.as_ref().map(|p| p.nom.as_str())) {
            // Redémarrée dans la journée : la conversation du jour reprend (`--resume`), sans préchauffage.
            journal::ecrire(&format!("la conversation du jour reprend (session {session})"));
            chef.session = Some(session);
            chef.reprendre = true;
            chef.memoire_a_donner = false;
            chef.seance_du = Some(journal::date());
        }
        if let Some((fil, voix)) = depart.annonce {
            chef.systeme(&fil);
            chef.dire_a_voix_haute(&voix);
        }
        if chef.micro.is_none() {
            chef.systeme("Aucun micro : branche un casque ou un micro USB — en attendant, écris tes demandes en bas.");
        }
        // reglages.json illisible : les valeurs d'office pour cette séance, le fichier intact (reglages.rs).
        if let Some(raison) = chef.reglages.illisible().map(str::to_string) {
            chef.systeme(&format!(
                "{} ne se lit pas ({raison}) : Eve démarre avec ses réglages d'office, ses privilèges compris, et n'y \
                 écrira rien tant qu'il ne se lira pas. Corrige-le, ou retire-le pour repartir de ceux d'office.",
                chef.reglages.chemin().display()
            ));
            chef.dire("Mes réglages ne se lisent pas : je démarre avec ceux d'office. Le détail est à l'écran.");
        }
        if chef.espace.constructo && chef.lien.d_essai() {
            journal::ecrire(&format!("Constructo AI : le serveur d'essai {}", chef.lien.serveur));
        }
        if chef.espace.constructo && !chef.connexion.connectee() {
            chef.systeme(
                "Eve n'est pas encore reliée à Constructo AI : « Se connecter à Constructo AI », dans ses réglages, ouvre \
                 sa page de connexion (le courriel et le mot de passe de ton entreprise).",
            );
            // Le premier lancement après l'installation, qui le demande (une fois : la variable ne vit que pour lui).
            if std::env::var_os("EVE_RELIER_CONSTRUCTO").is_some() {
                chef.connecter();
            }
        }
        chef
    }

    /// La boucle : un événement, sa suite, le portrait publié s'il a changé.
    pub fn mener(mut self, rx: Receiver<Evt>) {
        self.publier(true);
        while let Ok(evt) = rx.recv() {
            let tic = matches!(evt, Evt::Tic);
            self.traiter(evt);
            if self.quitter {
                break;
            }
            self.publier(tic);
        }
        if let Some(r) = self.relance.take() {
            journal::ecrire(&format!("mise à jour abandonnée, Eve se ferme avant : {}", r.quoi()));
        }
        journal::ecrire("Eve se ferme.");
        self.voix.taire();
        if let Some(c) = self.cerveau.take() {
            c.arreter_et_attendre();
        }
    }

    fn traiter(&mut self, evt: Evt) {
        match evt {
            Evt::Touche(true) => self.parler_debut(),
            Evt::Touche(false) => self.parler_fin(),
            Evt::Ui { action, valeur } => self.geste(&action, &valeur),
            Evt::Piece { nom, chemin, taille } => {
                journal::ecrire(&format!("pièce jointe : {}", chemin.display()));
                self.joindre(nom, chemin, taille);
            }
            Evt::Scan { nom, chemin, pages, but, sorte, taille } => self.scan(nom, chemin, pages, &but, &sorte, taille),
            Evt::Oreille(Ok(d)) => self.oreille_etat = ("prete", d),
            Evt::Oreille(Err(e)) => {
                self.oreille_etat = ("erreur", e.clone());
                self.systeme(&format!("L'oreille ne s'ouvre pas ({e}) : écris tes demandes en bas."));
            }
            Evt::Transcrit { texte, secondes_audio, secondes_calcul, continue_, depuis } => {
                self.transcriptions = self.transcriptions.saturating_sub(1);
                journal::ecrire(&format!(
                    "entendu ({secondes_audio:.1} s de parole, {secondes_calcul:.1} s) : {texte}"
                ));
                let t = parole::nettoyer_transcription(&texte);
                if t.is_empty() {
                    if !continue_ {
                        self.dire("Je n'ai pas compris. Recommence en tenant la touche.");
                    }
                } else if continue_ && !self.attentes.is_empty() && parole::reponse_d_accord(&t).is_none() {
                    // L'écoute continue entend tout (le téléphone, le bureau) : ce qui n'est pas une réponse s'ignore.
                    journal::ecrire("ignoré : pas une réponse à la demande d'accord");
                } else if continue_ && let Some(reste) = parole::appel_a_eve(&t) {
                    // Dans la fenêtre d'après « Eve », la personne redit « Eve, … » : lu comme un appel — le nom s'ôte
                    // (« Eve, scanne cette facture » ouvre la caméra), et « Eve » seul redemande « Oui ? ».
                    if let Some(reste) = self.passer_l_oreille(reste, secondes_audio, secondes_calcul, depuis) {
                        self.apres_eve(reste, depuis);
                    }
                } else if let Some(t) = self.passer_l_oreille(t, secondes_audio, secondes_calcul, depuis) {
                    self.phrase(t, Some(depuis));
                }
            }
            Evt::EveilEntendu => {
                self.transcriptions += 1;
                journal::ecrire("« Eve » entendu");
                // « Eve, … » pendant qu'elle parle : elle se tait aussitôt.
                if self.parle {
                    self.couper();
                }
                // La cloche met le micro en sourdine un instant, ce qui efface la phrase en cours : la personne qui
                // enchaîne déjà sa demande n'a pas de cloche (sinon le début de sa demande se perdait).
                if !self.micro.as_ref().is_some_and(Micro::en_parole) {
                    self.son(Son::Eveil);
                }
            }
            Evt::Eveil { demande, depuis } => {
                self.transcriptions = self.transcriptions.saturating_sub(1);
                journal::ecrire(&format!("demande après « Eve » : {demande}"));
                if let Some(d) = self.passer_l_oreille(parole::nettoyer_transcription(&demande), 0.0, 0.0, depuis) {
                    self.apres_eve(d, depuis);
                }
            }
            Evt::Claude { generation, message } => {
                if generation == self.generation {
                    self.claude(&message);
                }
            }
            Evt::ClaudeFini { generation } => {
                if generation == self.generation {
                    self.claude_fini();
                }
            }
            Evt::Voix { liste, choisie } => {
                self.voix_liste = liste;
                self.voix_choisie = choisie;
            }
            Evt::VoixEntendue(id) => {
                self.suivre_la_voix(id, true);
                let e = self.passages.voix(id, true);
                self.montrer(e);
            }
            Evt::VoixFinie(id) => {
                if id == self.parole_id {
                    self.parle = false;
                    self.fin_parole = Some(Instant::now());
                }
                if self.oui == Some(id) {
                    self.oui = None;
                    self.ecoute_suite = Some(Instant::now() + SUITE_DELAI);
                }
                self.suivre_la_voix(id, false);
                let e = self.passages.voix(id, false);
                self.montrer(e);
            }
            Evt::VoixErreur(e) => {
                // La phrase suivante suit son cours (son VoixFinie viendra) : l'état de la parole ne change pas ici.
                if self.erreur_voix.is_none_or(|t| t.elapsed() > Duration::from_secs(60)) {
                    self.erreur_voix = Some(Instant::now());
                    self.systeme(&format!("La voix a échoué : {e}"));
                }
            }
            Evt::Tic => self.tic(),
            Evt::Jeton(j) => {
                self.base = if j.is_some() {
                    ("inconnue", "connexion à la base…".into())
                } else {
                    ("absente", "aucun jeton d'Adjointe valable sur ce poste".into())
                };
                self.jeton = Some(j);
                if self.erreur.is_none() && self.cerveau.is_none() {
                    self.lancer_cerveau();
                }
            }
            Evt::Nouvelle(n) => self.nouvelle(n),
            Evt::RendezVous(r) => {
                if self.reglages.alertes.rendez_vous {
                    let m = r.minutes.unwrap_or(1).max(1);
                    let texte = format!("Dans {m} minute{} : {}", if m > 1 { "s" } else { "" }, oral(&r.titre));
                    self.ajouter("alerte", texte.clone(), Vec::new());
                    self.alertes.push_back(format!("Rappel : {texte}."));
                }
            }
            Evt::Agenda(a) => self.agenda = a,
            Evt::Calendrier { lu, jours } => {
                self.calendrier = jours;
                self.calendrier_lu = lu;
            }
            Evt::Retour(r) => self.retour_de_connexion(r),
            Evt::Connexion { tour, etape } => self.etape_de_connexion(tour, etape),
        }
    }

    fn geste(&mut self, action: &str, valeur: &Value) {
        match (action, valeur.as_str()) {
            ("parler_debut", _) => self.parler_debut(),
            ("parler_fin", _) => self.parler_fin(),
            ("texte", Some(t)) if !t.trim().is_empty() => self.phrase(t.trim().to_string(), None),
            ("confirmer" | "annuler", _) => {
                // Le numéro de la demande affichée : un double clic, ou une demande remplacée entre-temps, ne valent
                // pas pour la suivante.
                if self.attentes.front().is_some_and(|a| valeur.as_u64() == Some(a.numero)) {
                    self.decider(action == "confirmer");
                }
            }
            ("arreter", _) => self.arreter(),
            ("nouvelle", _) => {
                self.nouvelle_conversation("— Nouvelle conversation : la mémoire repart de zéro —", true)
            }
            ("voix", Some(id)) => {
                self.changer_reglage("voix", json!(id));
                // Déjà la voix du fichier, mais pas celle qui parle (une voix de repli : Piper a flanché) : elle revient.
                if self.reglages.voix == id {
                    self.changer_de_voix();
                }
            }
            // Le prénom tapé dans les réglages : ébarbé, quarante caractères au plus.
            ("qui", Some(q)) if !q.trim().is_empty() => {
                let q: String = q.trim().chars().take(40).collect();
                self.changer_reglage("qui", json!(q));
            }
            // Le site de Constructo AI, cliqué en haut de la fenêtre.
            ("site", _) => self.ouvrir_dans_le_navigateur(connexion::SITE.to_string()),
            // Un lien d'une réponse, cliqué dans la fenêtre : seulement `https://` (une adresse du poste, même en
            // `http://127.0.0.1`, ne s'ouvre jamais d'une réponse, que pourrait souffler une page lue par Claude).
            ("ouvrir_lien", Some(url)) if url.starts_with("https://") => {
                self.ouvrir_dans_le_navigateur(url.to_string())
            }
            // Le thème de la fenêtre native, choisi dans ses réglages : l'un de ceux qu'elle offre.
            ("theme", Some(t)) if crate::reglages::THEMES.iter().any(|x| x.0 == t) => {
                self.changer_reglage("theme", json!(t))
            }
            // Un profil, choisi dans le menu de la barre ou dans les réglages (son nom ; celui d'office y ramène).
            ("profil", Some(nom)) => self.choisir_le_profil(nom),
            // Le modèle et l'effort de Claude, choisis dans la rubrique « Cerveau » des réglages : ceux qu'Eve offre.
            ("modele", Some(m)) if crate::reglages::MODELES.iter().any(|x| x.0 == m) => {
                self.choisir_le_cerveau("modele", m);
            }
            ("effort", Some(e)) if crate::reglages::EFFORTS.iter().any(|x| x.0 == e) => {
                self.choisir_le_cerveau("effort", e);
            }
            ("piece_retirer", Some(id)) => {
                if let Some(i) = self.pieces.iter().position(|p| p.id == id) {
                    let p = self.pieces.remove(i);
                    let _ = std::fs::remove_file(&p.chemin);
                }
            }
            // Un fichier que la fenêtre native n'a pas joint (son fil) : le refus s'écrit au fil, comme dans la page.
            ("piece_erreur", Some(e)) => self.systeme(e),
            ("eveil", _) => self.changer_reglage("eveil", json!(valeur.as_bool().unwrap_or(true))),
            ("reglage", _) => {
                let cle = valeur.get("cle").and_then(Value::as_str).unwrap_or("");
                match valeur.get("valeur").and_then(Value::as_bool) {
                    Some(v) if cle == "demarrage" => self.demarrer_avec_windows(v),
                    Some(v) if INTERRUPTEURS.contains(&cle) => self.changer_reglage(cle, json!(v)),
                    _ => {}
                }
            }
            ("camera", _) => {
                let ouverte = valeur.as_bool().unwrap_or(false);
                // Fermée sans rien classer : ce qu'on avait dit en l'ouvrant ne vaut plus. (Un classement arrive avant :
                // la fenêtre ne ferme la caméra qu'une fois la numérisation reçue.)
                if !ouverte {
                    self.intentions_scan.clear();
                }
                self.camera = ouverte;
            }
            ("camera_erreur", Some(e)) => {
                journal::ecrire(&format!("caméra : {e}"));
                self.systeme(e);
                self.dire(e);
            }
            ("reprendre", Some(id)) => self.reprendre_conversation(id),
            ("ouvrir_brouillon", Some(id)) => self.ouvrir_brouillon(id),
            ("ouvrir_fichier", Some(c)) => self.ouvrir_fichier(c),
            ("brouillon_erreur", Some(e)) => self.systeme(&format!("Le brouillon ne s'ouvre pas dans Outlook : {e}")),
            // Le bouton des réglages : « Se connecter à Constructo AI » (déjà reliée : rebrancher), « Se déconnecter ».
            ("constructo_connecter", _) => self.connecter(),
            ("constructo_deconnecter", _) => self.deconnecter(),
            ("navigateur_erreur", Some(e)) => self.systeme(&format!("Le navigateur ne s'ouvre pas : {e}")),
            ("fenetre_fermee", _) => self.fenetre_fermee = true,
            ("quitter", _) => self.quitter = true,
            // `eve --mettre-a-jour` ou `eve --revenir` : redémarrer dans une autre version ; le bouton des réglages :
            // dans la précédente.
            ("mettre_a_jour", _) => {
                let r = Relance::du_geste(valeur);
                self.attendre_la_relance(r);
            }
            ("revenir", _) => self.attendre_la_relance(Relance::vers_la_precedente()),
            _ => {}
        }
    }

    /// Une relance demandée : Eve la fera à la fin du tour en cours ([`Self::mener_la_relance`]) ; une plus récente
    /// remplace celle qui attendait, tant qu'elle n'est pas dite.
    fn attendre_la_relance(&mut self, r: Result<Relance, String>) {
        match r {
            Ok(r) if self.relance_dite.is_none() => {
                journal::ecrire(&format!(
                    "mise à jour demandée : {} ; Eve redémarre à la fin du tour en cours",
                    r.quoi()
                ));
                let (vers, n) = (r.vers.clone(), self.nommee());
                let quand = if self.tours > 0 {
                    format!(", dès que {} aura fini", n.le)
                } else if self.agents_au_travail() {
                    ", quand mes agents auront fini".to_string()
                } else {
                    String::new()
                };
                self.systeme(&format!("Je vais redémarrer dans la version {vers}{quand}."));
                self.relance = Some(r);
            }
            Ok(r) => journal::ecrire(&format!("mise à jour écartée, une relance est déjà dite : {}", r.quoi())),
            Err(e) => {
                journal::ecrire(&format!("mise à jour refusée : {e}"));
                self.systeme(&format!("La mise à jour n'est pas partie : {e}."));
            }
        }
    }

    /// La relance attend qu'Eve soit libre — ni tour, ni demande en attente, ni accord, ni parole, ni écoute : jamais
    /// au milieu d'une réponse —, puis elle le dit, lance le gardien et se ferme comme au geste « quitter ».
    fn mener_la_relance(&mut self, maintenant: Instant) {
        let Some(r) = &self.relance else { return };
        let Some(dite) = self.relance_dite else {
            // Des agents au travail : la séance qui s'arrête les arrêterait ; la relance attend leur fin, et leur
            // compte rendu.
            let libre = self.tours == 0
                && self.en_attente.is_empty()
                && self.attentes.is_empty()
                && !self.parle
                && !self.ecoute
                && self.transcriptions == 0
                && self.ecoute_suite.is_none()
                && self.alerte_a_dire.is_none()
                && !self.agents_au_travail();
            if libre {
                let (fil, voix) = r.annonce();
                self.systeme(&fil);
                self.dire_a_voix_haute(voix);
                self.relance_dite = Some(maintenant);
            }
            return;
        };
        // La phrase dite (dix secondes au plus) ; une autre mise à jour en cours, deux minutes au plus.
        if self.parle && maintenant.duration_since(dite) < Duration::from_secs(10) {
            return;
        }
        match mise_a_jour::lancer_le_gardien(r, self.session.as_deref()) {
            Ok(pid) => {
                journal::ecrire(&format!("mise à jour : Eve se ferme pour la relance (gardien {pid}) — {}", r.quoi()));
                self.relance = None;
                self.quitter = true;
            }
            Err(Empeche::Occupe(_)) if maintenant.duration_since(dite) < Duration::from_secs(120) => {}
            Err(e) => {
                journal::ecrire(&format!("mise à jour : le gardien ne part pas ({e})"));
                self.systeme(&format!("Je n'ai pas pu redémarrer ({e}) : je reste dans cette version."));
                self.dire_a_voix_haute("Je n'ai pas pu redémarrer : je reste dans cette version.");
                self.relance = None;
                self.relance_dite = None;
            }
        }
    }

    /// La marque de santé : le chef tourne depuis cinq secondes et la fenêtre a dessiné une image (sans fenêtre, le
    /// chef seul) — le gardien d'une relance l'attend pour garder la nouvelle version. Puis, si c'en est une, Eve dit
    /// sa nouvelle version.
    fn marque_de_sante(&mut self, maintenant: Instant) {
        let depuis = maintenant.duration_since(self.demarre);
        if self.marque_posee || depuis < mise_a_jour::SANTE_APRES || (self.avec_fenetre && !self.a_eu_client) {
            return;
        }
        self.marque_posee = true;
        mise_a_jour::poser_la_marque(depuis);
        if let Some((fil, voix)) = self.a_la_sante.take() {
            self.systeme(&fil);
            self.dire_a_voix_haute(&voix);
        }
    }

    /// La version précédente des réglages, relue toutes les cinq secondes : le registre change sans qu'Eve le sache
    /// (son gardien y inscrit sa version une fois la marque posée, `eve --mettre-a-jour` y inscrit une version posée
    /// à la main).
    fn relire_la_precedente(&mut self, maintenant: Instant) {
        if maintenant.duration_since(self.precedente_lue) >= Duration::from_secs(5) {
            self.precedente_lue = maintenant;
            self.precedente = mise_a_jour::precedente_offerte();
        }
    }

    /// Un réglage changé dans la fenêtre : il s'inscrit dans reglages.json, puis tout suit le fichier
    /// ([`Self::suivre_les_reglages`]) ; s'il ne peut pas s'y inscrire, rien ne change, et la personne sait pourquoi.
    fn changer_reglage(&mut self, cle: &str, valeur: Value) {
        let avant = self.reglages.clone();
        if let Err(e) = self.reglages.changer(cle, valeur) {
            journal::ecrire(&format!("réglage « {cle} » : {e}"));
            self.systeme(&e);
        }
        self.suivre_les_reglages(&avant);
    }

    /// reglages.json changé hors d'Eve (à la main, par l'installateur, par l'Assistante elle-même) : relu entre deux
    /// demandes — un réglage changé en plein travail attend sa fin (« qui » ouvre une nouvelle conversation, qui
    /// couperait le tour).
    fn relire_les_reglages(&mut self) {
        if self.tours > 0 || self.parle || self.ecoute || !self.attentes.is_empty() {
            return;
        }
        if let Some(avant) = self.reglages.relire() {
            self.suivre_les_reglages(&avant);
        }
    }

    /// Les réglages ont changé — la mémoire n'est qu'une copie de reglages.json, qu'un geste de la fenêtre ou le
    /// fichier lui-même vient de changer : ce qui en dépend suit (le micro, la voix, la touche, la fenêtre, la personne
    /// devant le poste), et la personne sait ce qui attend le prochain démarrage, ou que le fichier ne se lit plus.
    fn suivre_les_reglages(&mut self, avant: &Reglages) {
        match (avant.illisible(), self.reglages.illisible()) {
            (None, Some(raison)) => {
                let m = format!(
                    "{} ne se lit plus ({raison}) : Eve garde les réglages qu'elle en avait, et n'y écrira rien tant \
                     qu'il ne se lira pas.",
                    self.reglages.chemin().display()
                );
                journal::ecrire(&m);
                self.systeme(&m);
            }
            (Some(_), None) => {
                journal::ecrire("reglages.json se lit de nouveau");
                self.systeme("reglages.json se lit de nouveau : Eve reprend ses réglages.");
            }
            _ => {}
        }
        let cles = crate::reglages::changements(avant, &self.reglages);
        if cles.is_empty() {
            return;
        }
        journal::ecrire(&format!("réglages : {}", cles.join(", ")));
        let r = self.reglages.clone();
        for cle in &cles {
            match cle.as_str() {
                "eveil" => {
                    if let Some(m) = &self.micro {
                        m.ecoute_continue(r.eveil);
                    }
                    self.systeme(if r.eveil {
                        "Mot d'éveil : Eve écoute ; dis « Eve… »."
                    } else {
                        "Mot d'éveil arrêté : la touche seulement."
                    });
                }
                "sourdine" => {
                    // « Oui ? » en train de se dire : la sourdine le coupe, et sa fin n'armera plus l'écoute de la
                    // suite — elle s'arme ici.
                    let attendait_la_suite = r.sourdine && self.oui.is_some();
                    if r.sourdine {
                        self.couper();
                    }
                    if attendait_la_suite {
                        self.ecoute_suite = Some(Instant::now() + SUITE_DELAI);
                    }
                    self.systeme(if r.sourdine {
                        "Sourdine : Eve répond par écrit."
                    } else {
                        "Sourdine retirée : Eve répond à voix haute."
                    });
                }
                "devant" => fenetre::premier_plan(r.devant),
                "historique" => self.systeme(if r.historique {
                    "L'historique des conversations est gardé sur ce poste."
                } else {
                    "L'historique ne garde plus rien ; ce qui y est reste."
                }),
                "touche" => touche::changer(r.touche().0),
                "voix" => self.changer_de_voix(),
                "qui" => {
                    let titre = if r.qui.is_empty() {
                        "— Nouvelle conversation —".to_string()
                    } else {
                        format!("— {} est devant le poste : nouvelle conversation —", r.qui)
                    };
                    // Les conversations des autres profils étaient celles de la personne d'avant.
                    self.laisses.oublier();
                    self.nouvelle_conversation(&titre, false);
                }
                // Un profil choisi, ajouté, retiré, ou qui change de dossier ; un autre modèle, un autre effort : Claude
                // Code repart avec eux entre deux demandes.
                "profil" | "profils" | "modele" | "effort" => {
                    self.claude_a_suivre = true;
                    self.claude_change_a = Instant::now();
                }
                _ => {}
            }
        }
        let au_demarrage: Vec<&str> =
            cles.iter().map(String::as_str).filter(|c| crate::reglages::au_prochain_demarrage(c)).collect();
        if !au_demarrage.is_empty() {
            self.systeme(&format!(
                "Changé dans reglages.json, pour le prochain démarrage d'Eve : {}.",
                au_demarrage.join(", ")
            ));
        }
    }

    // ---- Les profils -------------------------------------------------------------------------------------------

    /// Le dossier où travaille Claude Code : celui du profil actif, d'office celui d'Eve.
    fn dossier_de_claude(&self) -> Option<PathBuf> {
        self.profil.as_ref().map(|p| p.dossier.clone()).or_else(|| self.dossier.clone())
    }

    /// Le nom du profil actif (celui d'office : le nom d'Eve).
    fn nom_du_profil(&self) -> &str {
        self.profil.as_ref().map_or(nom_affiche(), |p| p.nom.as_str())
    }

    /// Un profil choisi dans la fenêtre (le menu de la barre, la rubrique des réglages) : il s'inscrit dans
    /// reglages.json, comme le fait `eve --profil`, et Eve y passe dès qu'aucun tour ne court. Un profil qui ne peut
    /// pas servir ne s'inscrit pas : la personne sait pourquoi.
    fn choisir_le_profil(&mut self, nom: &str) {
        match profils::trouver(&self.reglages, nom_affiche(), nom) {
            Err(raison) => self.systeme(&raison),
            Ok(choix) => {
                self.changer_reglage("profil", json!(choix.as_ref().map_or("", |p| p.nom.as_str())));
                // Même déjà inscrit (son dossier revenu après un refus), il se suit.
                self.claude_a_suivre = true;
                self.claude_change_a = Instant::now();
                if let Some(quand) = self.quand_libre()
                    && !profils::meme_profil(choix.as_ref(), self.profil.as_ref())
                {
                    let nom = choix.as_ref().map_or(nom_affiche(), |p| p.nom.as_str());
                    self.systeme(&format!("J'irai au profil {nom} {quand}."));
                }
            }
        }
    }

    /// Un modèle ou un effort choisis dans la fenêtre (la rubrique « Cerveau ») : ils s'inscrivent dans reglages.json,
    /// comme le fait `eve --modele`, et Claude Code repart avec eux dès qu'il est libre ([`Self::suivre_claude`]).
    fn choisir_le_cerveau(&mut self, cle: &str, valeur: &str) {
        self.changer_reglage(cle, json!(valeur));
        if let Some(quand) = self.quand_libre() {
            let modele = crate::reglages::nom_du_modele(&self.reglages.modele);
            let effort = crate::reglages::nom_de_l_effort(&self.reglages.effort);
            self.systeme(&format!("Je passerai à {modele}, effort {effort}, {quand}."));
        }
    }

    /// Ce qu'attend un changement de Claude Code (un autre profil, un autre cerveau, une mise à jour), s'il attend : la
    /// fin de la demande en cours, ou celle des agents qui travaillent en arrière-plan.
    fn quand_libre(&self) -> Option<&'static str> {
        if self.tours > 0 && !self.prechauffage {
            Some("à la fin de la demande en cours")
        } else if self.agents_au_travail() {
            Some("quand mes agents auront fini")
        } else {
            None
        }
    }

    /// Des agents de Claude travaillent encore, ou l'un d'eux a fini sans que Claude en ait encore rendu compte : la
    /// séance de Claude Code ne s'arrête pas sous eux.
    fn agents_au_travail(&self) -> bool {
        !self.agents.is_empty() || self.compte_rendu_attendu.is_some_and(|t| t.elapsed() < COMPTE_RENDU_MAX)
    }

    /// Le profil, le modèle ou l'effort de Claude ont changé : dès qu'aucun tour ne court ni n'attend un accord, que la
    /// personne ne parle pas et qu'aucun agent ne travaille, Claude Code repart — dans le profil choisi
    /// ([`Self::passer_au_profil`]), ou avec le modèle et l'effort choisis, la conversation reprise
    /// ([`Self::changer_de_cerveau`]). Un profil qui ne peut plus servir, Eve le dit et revient (ou reste) dans son
    /// profil d'office. Ce qui attendait part ensuite.
    fn suivre_claude(&mut self) {
        let occupee = self.tours > 0
            || !self.attentes.is_empty()
            || self.ecoute
            || self.transcriptions > 0
            || self.agents_au_travail();
        if !self.claude_a_suivre || occupee || self.claude_change_a.elapsed() < REGROUPER {
            return;
        }
        self.claude_a_suivre = false;
        let voulu = match profils::trouver(&self.reglages, nom_affiche(), &self.reglages.profil) {
            Ok(p) => p,
            Err(raison) => {
                let ou = if self.profil.is_some() { "je reviens à" } else { "je reste dans" };
                let m = format!("{raison} Alors {ou} mon profil d'office, {}.", nom_affiche());
                journal::ecrire(&format!("profil : {m}"));
                self.systeme(&m);
                None
            }
        };
        let cerveau = (self.reglages.modele.clone(), self.reglages.effort.clone());
        if !profils::meme_profil(voulu.as_ref(), self.profil.as_ref()) {
            self.passer_au_profil(voulu);
        } else if self.cerveau.is_some() && self.lance_avec != cerveau {
            self.changer_de_cerveau();
        } else {
            self.suivant();
        }
    }

    /// Claude Code s'arrête et repart dans le dossier du profil `vers` (son `CLAUDE.md`), sa consigne avec lui
    /// (`cerveau.rs`). Le profil quitté laisse sa séance et sa conversation de l'historique ; celui où Eve arrive
    /// reprend les siennes (`--resume`), s'il en a laissé. Les demandes qui attendaient partent dans le nouveau
    /// profil ; ce qui se disait finit de se dire, puis Eve l'annonce.
    fn passer_au_profil(&mut self, vers: Option<Profil>) {
        let reprise = self.echanger_les_profils(vers);
        let nom = self.nom_du_profil().to_string();
        let comment = if reprise { ", sa conversation reprise" } else { ", une conversation neuve" };
        let dossier = self.dossier_de_claude().unwrap_or_default();
        journal::ecrire(&format!("profil : {nom} ({}){comment}", dossier.display()));
        self.arreter_pour_repartir();
        let suite = if reprise { " : sa conversation reprend" } else { "" };
        self.ajouter("systeme", format!("— Profil {nom}{suite} —"), Vec::new());
        self.dire_a_voix_haute(&format!("Je passe au profil {nom}."));
        self.repartir();
    }

    /// Claude Code repart avec le modèle et l'effort des réglages, dans le même profil : la conversation continue
    /// (`--resume`), sous un autre cerveau. Les demandes qui attendaient partent avec lui.
    fn changer_de_cerveau(&mut self) {
        let modele = crate::reglages::nom_du_modele(&self.reglages.modele).to_string();
        let effort = crate::reglages::nom_de_l_effort(&self.reglages.effort).to_string();
        journal::ecrire(&format!(
            "cerveau : {} (effort {}), la conversation reprise",
            self.reglages.modele, self.reglages.effort
        ));
        self.arreter_pour_repartir();
        self.reprendre = self.session.is_some();
        self.ajouter("systeme", format!("— {modele}, effort {effort} : la conversation continue —"), Vec::new());
        // À voix haute, le nom sans sa version : « Opus », pas « Opus cinq point cinq ».
        let dit = modele.split(' ').next().unwrap_or(&modele).to_string();
        self.dire_a_voix_haute(&format!("Je passe à {dit}, effort {effort}."));
        self.repartir();
    }

    /// Claude Code s'arrête pour repartir aussitôt (un autre profil, un autre cerveau). Ce qui attend reste, un
    /// préchauffage compris (il porte peut-être la mémoire des derniers échanges) : la nouvelle séance le prend, et n'en
    /// ajoute pas d'autre.
    fn arreter_pour_repartir(&mut self) {
        if let Some(c) = self.cerveau.take() {
            c.arreter();
        }
        self.init_ok = false;
        self.branchement = None;
        self.constructo_branche = false;
        self.prechauffage = false;
        self.interrompu = false;
        self.chrono = None;
        self.activite.clear();
        self.vus.clear();
        self.signatures.clear();
        self.en_route.clear();
        self.en_cours.clear();
        self.flux.nouveau_tour();
        self.rechauffer = false;
        self.rechauffages = 0;
        self.agents.clear();
        self.compte_rendu_attendu = None;
    }

    /// La nouvelle séance de Claude Code part, si Eve a de quoi la lancer ([`Self::arreter_pour_repartir`]).
    fn repartir(&mut self) {
        self.generation += 1;
        if self.dossier_de_claude().is_some() && self.claude.is_some() {
            self.erreur = None;
            self.lancer_cerveau();
        }
    }

    /// Le profil actif laisse sa séance et sa conversation de l'historique ([`Laisses`]) ; le profil `vers` devient
    /// l'actif, avec ce qu'il avait laissé — une séance que le prochain lancement reprend (`--resume`), sa
    /// conversation —, sinon une conversation neuve. Rend vrai s'il reprend une séance.
    fn echanger_les_profils(&mut self, vers: Option<Profil>) -> bool {
        let quitte = Laisse {
            dossier: self.dossier_de_claude().unwrap_or_default(),
            session: self.session.take(),
            conversation: std::mem::take(&mut self.conversation),
        };
        let de = self.profil.take().map(|p| p.nom);
        self.profil = vers;
        let dossier = self.dossier_de_claude().unwrap_or_default();
        let vers = self.profil.as_ref().map(|p| p.nom.as_str());
        let retrouve = self.laisses.echanger(de.as_deref(), quitte, vers, &dossier);
        (self.session, self.conversation) = match retrouve {
            Some(l) => (l.session, l.conversation),
            None => (None, journal::horodatage()),
        };
        self.reprendre = self.session.is_some();
        self.reprendre
    }

    /// Les profils pour la fenêtre (son menu, ses réglages) : celui d'office d'abord, puis ceux de reglages.json.
    fn profils_publies(&self) -> Vec<Value> {
        let office = nom_affiche();
        let d_office = self.dossier.as_ref().map(|d| d.display().to_string());
        std::iter::once(json!({ "nom": office, "dossier": d_office, "office": true }))
            .chain(
                profils::liste(&self.reglages, office)
                    .map(|p| json!({ "nom": p.nom, "dossier": p.dossier.display().to_string(), "office": false })),
            )
            .collect()
    }

    /// La voix des réglages prend la parole, si ce n'est pas déjà elle qui parle.
    fn changer_de_voix(&mut self) {
        if self.voix_choisie != self.reglages.voix {
            let id = self.reglages.voix.clone();
            self.voix.choisir(&id);
            self.voix_choisie = id;
            self.dire("Voici ma nouvelle voix.");
        }
    }

    /// « Démarrer avec Windows » : le registre fait foi, rien dans reglages.json.
    fn demarrer_avec_windows(&mut self, v: bool) {
        match fenetre::demarrage_windows(v) {
            Ok(()) => {
                self.systeme(if v { "Eve démarrera avec Windows." } else { "Eve ne démarrera plus avec Windows." })
            }
            Err(e) => self.systeme(&format!("Le démarrage avec Windows n'a pas pu changer : {e}")),
        }
        self.demarrage_windows = fenetre::demarrage_windows_actif();
    }

    /// Ouvre un brouillon dans Outlook (la carte « brouillon ») : la personne le relit et clique « Envoyer » (règle 1).
    fn ouvrir_brouillon(&mut self, id: &str) {
        // Un identifiant d'Outlook (EntryID) n'a que des chiffres hexadécimaux : rien que PowerShell puisse interpréter.
        if id.len() < 16 || id.len() > 1024 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
            self.systeme("Ce brouillon n'a pas d'identifiant d'Outlook valable.");
            return;
        }
        let script = format!(
            "$ErrorActionPreference = 'Stop'; $o = New-Object -ComObject Outlook.Application; \
             $i = $o.Session.GetItemFromID('{id}'); $i.Display(); try {{ $i.GetInspector.Activate() }} catch {{}}"
        );
        let rapport = self.rapport.clone();
        std::thread::spawn(move || {
            let mut c = Command::new("powershell");
            c.args(["-NoProfile", "-NonInteractive", "-Command", &script])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::piped());
            cerveau::sans_fenetre(&mut c);
            let erreur = match c.output() {
                Ok(s) if s.status.success() => return,
                Ok(s) => String::from_utf8_lossy(&s.stderr).trim().to_string(),
                Err(e) => e.to_string(),
            };
            let valeur = Value::String(garde::ecourter(&erreur, 200));
            let _ = rapport.send(Evt::Ui { action: "brouillon_erreur".into(), valeur });
        });
        self.systeme("J'ouvre le brouillon dans Outlook.");
    }

    // ---- Constructo AI -----------------------------------------------------------------------------------------

    /// « Se connecter à Constructo AI » : l'enregistrement d'Eve si besoin, puis la page de connexion du serveur dans le
    /// navigateur du poste. Déjà reliée, le même geste la rebranche : un jeton neuf, que Claude Code reprend (le
    /// « Réessayer » d'une panne). Le serveur se parle dans un fil : le chef ne s'arrête jamais sur le réseau.
    fn connecter(&mut self) {
        if !self.espace.constructo {
            self.systeme(
                "Constructo AI n'est pas permis à cette Eve : ses privilèges (reglages.json, « constructo ») ne le \
                 donnent pas.",
            );
            return;
        }
        if self.connexion_occupee {
            self.systeme("La connexion à Constructo AI est déjà en route.");
            return;
        }
        self.constructo_erreur = None;
        if self.connexion.connectee() {
            journal::ecrire("Constructo AI : rebranché à la demande (un jeton neuf)");
            self.systeme("Je rebranche Constructo AI…");
            self.connexion.expire = 0;
            self.renouveler();
            return;
        }
        self.connexion_occupee = true;
        self.systeme("J'ouvre la page de connexion de Constructo AI dans ton navigateur…");
        let (lien, etat, port) = (self.lien.clone(), self.connexion.clone(), self.ecran.port);
        let repli = EtapeConnexion::Echec("la connexion a planté (voir eve.log)".into());
        self.en_fil(repli, move || match lien.preparer(&etat, port) {
            Ok((etat, en_cours, url)) => EtapeConnexion::Page { etat, en_cours, url },
            Err(e) => EtapeConnexion::Echec(e),
        });
    }

    /// Un fil qui parle au serveur, pour le tour de connexion du moment ; une panique y rend `repli` au lieu de laisser la
    /// connexion « en route » pour toujours.
    fn en_fil(&self, repli: EtapeConnexion, f: impl FnOnce() -> EtapeConnexion + Send + 'static) {
        let (tour, rapport) = (self.connexion_tour, self.rapport.clone());
        std::thread::spawn(move || {
            let etape = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or_else(|_| {
                journal::ecrire("un fil de la connexion à Constructo AI a planté");
                repli
            });
            let _ = rapport.send(Evt::Connexion { tour, etape });
        });
    }

    /// Le retour du navigateur : il doit rapporter le `state` de la connexion en cours ; le code part ensuite contre les
    /// jetons, dans un fil.
    fn retour_de_connexion(&mut self, r: Retour) {
        if !self.espace.constructo {
            return;
        }
        match self.lien.verifier_le_retour(self.connexion_en_cours.as_ref(), &r) {
            Ok(code) => {
                let Some(en_cours) = self.connexion_en_cours.take() else { return };
                self.connexion_occupee = true;
                self.systeme("Constructo AI a donné son accord ; je termine la connexion…");
                let (lien, etat) = (self.lien.clone(), self.connexion.clone());
                let repli = EtapeConnexion::Echec("la connexion a planté (voir eve.log)".into());
                self.en_fil(repli, move || match lien.echanger(&etat, &en_cours, &code) {
                    Ok(e) => EtapeConnexion::Jetons(e),
                    Err(e) => EtapeConnexion::Echec(e),
                });
            }
            Err(e) => {
                journal::ecrire(&format!("retour de connexion écarté : {e}"));
                // Le retour de la connexion en cours (il porte son `state`), mais refusé par le serveur ou trop tard :
                // elle prend fin. Un autre retour (une page rouverte, une adresse forgée) ne la touche pas : le vrai peut
                // encore venir.
                let le_sien = self
                    .connexion_en_cours
                    .as_ref()
                    .is_some_and(|c| !r.etat_oauth.is_empty() && connexion::egaux(&c.etat_oauth, &r.etat_oauth));
                if le_sien {
                    self.connexion_en_cours = None;
                    self.constructo_erreur = Some(e.clone());
                    self.systeme(&e);
                    self.dire("La connexion à Constructo AI n'a pas abouti. Le détail est à l'écran.");
                }
            }
        }
    }

    fn etape_de_connexion(&mut self, tour: u64, etape: EtapeConnexion) {
        if tour != self.connexion_tour {
            // Un fil d'avant la déconnexion. Des jetons qu'il rapporte sont déjà écrits sur le disque (un renouvellement
            // les écrit avant de servir) : l'état du moment, déconnecté, se réécrit par-dessus.
            journal::ecrire("réponse d'une connexion abandonnée (déconnexion entre-temps) : écartée");
            if matches!(etape, EtapeConnexion::Jetons(_))
                && let Err(e) = self.lien.enregistrer(&self.connexion)
            {
                journal::ecrire(&format!("la connexion oubliée ne s'écrit pas : {e}"));
            }
            return;
        }
        self.connexion_occupee = false;
        match etape {
            EtapeConnexion::Page { etat, en_cours, url } => {
                self.connexion = etat;
                self.connexion_en_cours = Some(en_cours);
                self.ouvrir_dans_le_navigateur(url);
            }
            EtapeConnexion::Jetons(e) => {
                let nouvelle = !self.connexion.connectee();
                self.connexion = e;
                self.renouveler_a = None;
                self.renouveler_delai = RENOUVELER_APRES;
                self.constructo_erreur = None;
                if nouvelle {
                    let compte = self.connexion.compte.clone();
                    journal::ecrire(&format!("relié à Constructo AI ({compte})"));
                    let au_compte = if compte.is_empty() { String::new() } else { format!(" (compte {compte})") };
                    self.systeme(&format!("Eve est reliée à Constructo AI{au_compte}."));
                    self.dire("Je suis reliée à Constructo AI.");
                } else {
                    journal::ecrire("jeton de Constructo AI renouvelé");
                }
                self.brancher_constructo();
            }
            EtapeConnexion::Echec(e) => {
                journal::ecrire(&format!("connexion à Constructo AI : {e}"));
                self.connexion_en_cours = None;
                self.constructo_erreur = Some(e.clone());
                self.systeme(&format!("La connexion à Constructo AI n'a pas abouti : {e}"));
                self.dire("La connexion à Constructo AI n'a pas abouti. Le détail est à l'écran.");
            }
            EtapeConnexion::Renouvellement(Refus::Definitif(raison)) => {
                journal::ecrire(&format!("renouvellement refusé par Constructo AI : {raison}"));
                self.oublier_la_connexion();
                self.constructo_erreur = Some(format!("La connexion a pris fin ({raison}) : reconnecte Eve."));
                self.systeme(&format!(
                    "La connexion à Constructo AI a pris fin ({raison}) : « Se connecter à Constructo AI », dans mes \
                     réglages, me relie de nouveau."
                ));
                self.dire("Ma connexion à Constructo AI a pris fin. Reconnecte-moi depuis mes réglages.");
            }
            EtapeConnexion::Renouvellement(Refus::Passager(raison)) => {
                let delai = self.renouveler_delai;
                journal::ecrire(&format!("renouvellement manqué ({raison}) : nouvel essai dans {} s", delai.as_secs()));
                self.renouveler_a = Some(Instant::now() + delai);
                self.renouveler_delai = (delai * 2).min(Duration::from_secs(15 * 60));
                if !self.constructo_branche || !self.connexion.acces_valable(connexion::maintenant()) {
                    self.constructo_erreur =
                        Some(format!("{raison} ; nouvel essai dans {} min.", delai.as_secs().div_ceil(60)));
                }
            }
        }
        // Une demande qui attendait les outils repart (avec eux, ou sans eux si la connexion a échoué).
        self.suivant();
    }

    /// Oublie les jetons (le fichier aussi) et retire Constructo AI de la séance ; l'enregistrement d'Eve reste. Un
    /// nouveau tour de connexion commence : ce qu'un fil encore en route rapportera sera écarté.
    fn oublier_la_connexion(&mut self) {
        self.connexion_tour += 1;
        self.connexion_occupee = false;
        self.renouveler_a = None;
        self.renouveler_delai = RENOUVELER_APRES;
        self.connexion = self.connexion.sans_jetons();
        if let Err(e) = self.lien.enregistrer(&self.connexion) {
            journal::ecrire(&format!("la connexion oubliée ne s'écrit pas : {e}"));
        }
        self.connexion_en_cours = None;
        self.constructo_erreur = None;
        self.constructo_branche = false;
        if self.init_ok {
            self.branchement = None;
            if let Some(Err(e)) = self.cerveau.as_ref().map(|c| c.brancher_mcp(self.lien.serveurs_mcp(None))) {
                journal::ecrire(&format!("Constructo AI ne se retire pas de Claude Code : {e}"));
            }
        }
    }

    fn deconnecter(&mut self) {
        if !self.connexion.connectee() && self.connexion_en_cours.is_none() && !self.connexion_occupee {
            self.systeme("Eve n'est pas reliée à Constructo AI.");
            return;
        }
        journal::ecrire("déconnecté de Constructo AI (à la demande)");
        self.oublier_la_connexion();
        self.systeme("Eve n'est plus reliée à Constructo AI : ses jetons sont effacés de ce poste.");
    }

    /// Remet Constructo AI à Claude Code avec le jeton du moment (après l'initialisation, une connexion, un
    /// renouvellement). Un jeton échu se renouvelle d'abord : le branchement suivra.
    fn brancher_constructo(&mut self) {
        if !self.espace.constructo || !self.init_ok || !self.connexion.connectee() {
            return;
        }
        let maintenant = connexion::maintenant();
        if !self.connexion.acces_valable(maintenant) {
            if self.connexion.a_renouveler(maintenant) {
                self.renouveler();
            }
            return;
        }
        let serveurs = self.lien.serveurs_mcp(Some(&self.connexion.acces));
        match self.cerveau.as_ref().map(|c| c.brancher_mcp(serveurs)) {
            Some(Ok(id)) => self.branchement = Some(Branchement { id, depuis: Instant::now(), retient: true }),
            Some(Err(e)) => journal::ecrire(&format!("Constructo AI ne se branche pas : {e}")),
            None => {}
        }
    }

    /// La réponse de Claude Code au branchement : le serveur a-t-il accepté le jeton ? Sa réponse peut citer la
    /// configuration du serveur, jeton compris : elle ne va jamais au journal telle quelle.
    fn branchement_fait(&mut self, reussie: bool, erreur: &str, corps: &Value) {
        self.branchement = None;
        let probleme =
            if reussie { connexion::erreur_du_branchement(corps) } else { Some(connexion::sans_porteur(erreur)) };
        match probleme {
            None => {
                self.constructo_branche = true;
                self.constructo_erreur = None;
                journal::ecrire("Constructo AI branché dans Claude Code");
            }
            Some(p) => {
                self.constructo_branche = false;
                journal::ecrire(&format!("Constructo AI refusé par Claude Code : {p}"));
                let jeton_refuse = p.contains("401") || p.to_lowercase().contains("unauthorized");
                if jeton_refuse && !self.connexion.renouvellement.is_empty() && self.renouveler_a.is_none() {
                    // Un jeton refusé (révoqué entre-temps) : un renouvellement tranche.
                    self.connexion.expire = 0;
                    self.renouveler();
                } else {
                    // Le serveur qui dort, le réseau : un nouvel essai plus tard, avec un jeton neuf que Claude Code
                    // reprend (le même, il pourrait le croire déjà branché).
                    let delai = self.renouveler_delai;
                    self.constructo_erreur = Some(format!(
                        "Constructo AI ne répond pas ({}) ; nouvel essai dans {} min.",
                        garde::ecourter(&p, 120),
                        delai.as_secs().div_ceil(60)
                    ));
                    self.connexion.expire = 0;
                    self.renouveler_a = Some(Instant::now() + delai);
                    self.renouveler_delai = (delai * 2).min(Duration::from_secs(15 * 60));
                }
            }
        }
        self.suivant();
    }

    /// Le renouvellement du jeton d'accès, dans un fil.
    fn renouveler(&mut self) {
        if self.connexion_occupee || self.connexion.renouvellement.is_empty() {
            return;
        }
        self.connexion_occupee = true;
        let (lien, etat) = (self.lien.clone(), self.connexion.clone());
        let repli = EtapeConnexion::Renouvellement(Refus::Passager("le renouvellement a planté (voir eve.log)".into()));
        self.en_fil(repli, move || match lien.renouveler(&etat) {
            Ok(e) => EtapeConnexion::Jetons(e),
            Err(r) => EtapeConnexion::Renouvellement(r),
        });
    }

    /// Au battement : le jeton qui arrive à échéance se renouvelle (une heure de vie, dix minutes de marge) ; une
    /// connexion laissée en plan dans le navigateur s'oublie après dix minutes ; un branchement sans réponse ne retient
    /// plus les demandes après 45 secondes.
    fn surveiller_la_connexion(&mut self, maintenant: Instant) {
        if !self.espace.constructo {
            return;
        }
        if self.connexion_en_cours.as_ref().is_some_and(|c| c.depuis.elapsed() > connexion::DELAI_CONNEXION) {
            self.connexion_en_cours = None;
            self.systeme(
                "La connexion à Constructo AI n'a pas été terminée dans le navigateur : recommence quand tu veux.",
            );
        }
        let attendre = self.renouveler_a.is_some_and(|t| maintenant < t);
        if !self.connexion_occupee && !attendre && self.connexion.a_renouveler(connexion::maintenant()) {
            self.renouveler_a = None;
            self.renouveler();
        }
        if let Some(b) = &mut self.branchement
            && b.retient
            && maintenant.duration_since(b.depuis) > BRANCHEMENT_MAX
        {
            b.retient = false;
            journal::ecrire(
                "le branchement de Constructo AI n'a pas répondu en 45 s : les demandes repartent sans lui",
            );
            self.suivant();
        }
    }

    /// Les outils de Constructo AI arrivent (le branchement, ou le renouvellement qui le précède) : une demande de la
    /// personne les attend.
    fn outils_en_route(&self) -> bool {
        self.espace.constructo
            && (self.branchement.as_ref().is_some_and(|b| b.retient)
                || (self.connexion_occupee
                    && self.connexion.connectee()
                    && !self.connexion.acces_valable(connexion::maintenant())))
    }

    /// Une page dans le navigateur du poste, dans un fil ; si elle ne s'ouvre pas, son adresse s'affiche (la page de
    /// connexion ne porte aucun secret : le vérificateur de PKCE reste dans Eve).
    fn ouvrir_dans_le_navigateur(&mut self, url: String) {
        let rapport = self.rapport.clone();
        std::thread::spawn(move || {
            if let Err(e) = connexion::ouvrir_dans_le_navigateur(&url) {
                let valeur = Value::String(format!("{e}. Copie cette adresse dans ton navigateur : {url}"));
                let _ = rapport.send(Evt::Ui { action: "navigateur_erreur".into(), valeur });
            }
        });
    }

    /// Ce que la fenêtre montre de Constructo AI : le privilège, la connexion, ses outils dans la séance, le compte, la
    /// panne, et une ligne qui dit tout cela.
    fn etat_constructo(&self) -> Value {
        let page_ouverte = self.connexion_en_cours.is_some();
        let etat = if page_ouverte || (self.connexion_occupee && !self.connexion.connectee()) {
            "en_cours"
        } else if self.connexion.connectee() {
            "connectee"
        } else {
            "deconnectee"
        };
        let compte = &self.connexion.compte;
        let reliee = if compte.is_empty() { "Reliée".to_string() } else { format!("Reliée au compte {compte}") };
        let detail = match etat {
            _ if !self.espace.constructo => {
                "Constructo AI n'est pas permis à cette Eve (reglages.json, « privileges »).".to_string()
            }
            "en_cours" if page_ouverte => PAGE_DE_CONNEXION_OUVERTE.to_string(),
            "en_cours" => "Eve parle à Constructo AI… (s'il dormait, une trentaine de secondes)".to_string(),
            "connectee" if self.constructo_branche => format!("{reliee} : ses outils sont prêts."),
            "connectee" if self.init_ok && self.constructo_erreur.is_none() => {
                format!("{reliee} ; ses outils arrivent…")
            }
            "connectee" => format!("{reliee}."),
            _ => "Pas encore reliée à Constructo AI.".to_string(),
        };
        json!({
            "permis": self.espace.constructo,
            "etat": etat,
            "connectee": self.connexion.connectee(),
            "branchee": self.constructo_branche,
            "compte": compte,
            "erreur": self.constructo_erreur,
            "detail": detail,
        })
    }

    // ---- La parole de la personne ----------------------------------------------------------------------------

    fn parler_debut(&mut self) {
        if self.ecoute {
            return;
        }
        // La touche dit la demande elle-même : la fenêtre ouverte après « Eve » se referme (sinon une phrase entendue
        // ensuite était prise pour une seconde demande).
        self.ecoute_suite = None;
        self.oui = None;
        match &self.micro {
            Some(m) => {
                m.debut();
                self.ecoute = true;
                self.debut_ecoute = Some(Instant::now());
            }
            None if !self.micro_signale => {
                self.micro_signale = true;
                self.dire("Je n'ai pas de micro. Branche ton casque, ou écris ta demande.");
            }
            None => {}
        }
    }

    fn parler_fin(&mut self) {
        if !self.ecoute {
            return;
        }
        let fin = Instant::now();
        self.ecoute = false;
        self.debut_ecoute = None;
        let Some(m) = &self.micro else { return };
        let audio = m.fin();
        let secondes = audio.len() as f32 / micro::CADENCE as f32;
        // Trop court : un raccourci clavier (Ctrl+C), pas une phrase.
        if secondes < 0.35 {
            return;
        }
        if micro::rms(&audio) < 0.0025 {
            self.dire("Je n'ai rien entendu.");
            return;
        }
        if self.oreille_etat.0 != "prete" {
            self.systeme("L'oreille n'est pas encore prête : écris ta demande, ou réessaie dans un instant.");
            return;
        }
        if self.oreille.send(Demande::Commande { audio, continue_: false, depuis: fin }).is_ok() {
            self.transcriptions += 1;
            self.son(Son::Eveil);
        } else {
            self.oreille_perdue();
        }
    }

    /// « Eve, … » entendu (la phrase finie à `depuis`) : la demande qui suit le nom. « Eve » seul : « Oui ? », puis
    /// l'écoute de la suite.
    fn apres_eve(&mut self, demande: String, depuis: Instant) {
        if !demande.is_empty() {
            self.phrase(demande, Some(depuis));
        } else if self.micro.as_ref().is_some_and(Micro::en_parole) {
            // La personne enchaîne déjà : pas de « Oui ? » par-dessus sa phrase.
            self.ecoute_suite = Some(Instant::now() + SUITE_DELAI);
        } else if self.reglages.sourdine {
            // En sourdine, « Oui ? » s'écrit sans se dire : aucune parole ne finira pour armer l'écoute de la suite
            // (`Evt::VoixFinie`), elle s'arme tout de suite. Sans cela, la demande dite après la pause s'ignorait, et
            // `oui` restait pris sur une parole déjà finie.
            self.dire("Oui ?");
            self.ecoute_suite = Some(Instant::now() + SUITE_DELAI);
        } else {
            self.dire("Oui ?");
            self.oui = Some(self.parole_id);
        }
    }

    /// Une phrase de la personne : dite (`fin_de_parole` : l'instant où elle a fini de parler) ou écrite (`None`).
    fn phrase(&mut self, t: String, fin_de_parole: Option<Instant>) {
        let a_la_voix = fin_de_parole.is_some();
        journal::ecrire(&format!("demande : {t}"));
        // Une demande est arrivée (dite, tapée, ou prise dans la fenêtre) : la fenêtre d'après « Eve » se referme, et
        // un « Oui ? » encore en train de se dire ne la rouvrira pas.
        self.ecoute_suite = None;
        self.oui = None;
        if !self.attentes.is_empty() {
            self.ajouter("toi", t.clone(), Vec::new());
            match parole::reponse_d_accord(&t) {
                Some(_) if self.tete_depuis.elapsed() < ACCORD_GARDE => self.annoncer_accord(),
                Some(oui) => self.decider(oui),
                None => self.dire("Pour laisser partir l'écriture, dis « je confirme ». Sinon, dis « annule »."),
            }
            return;
        }
        // La voix sans Claude : « la suite », « répète », la sourdine (avant l'arrêt : « mode silence » la met).
        if parole::demande_la_suite(&t) {
            self.lire_la_suite();
            return;
        }
        if parole::demande_de_repeter(&t) {
            self.repeter();
            return;
        }
        if let Some(sourdine) = parole::reglage_de_la_sourdine(&t) {
            self.changer_reglage("sourdine", json!(sourdine));
            if sourdine {
                self.couper();
                self.systeme("Sourdine : je réponds par écrit. Dis « parle » pour m'entendre.");
            } else {
                self.dire("D'accord, je parle de nouveau.");
            }
            return;
        }
        if parole::demande_d_annuler(&t) {
            self.ajouter("toi", t, Vec::new());
            self.annuler_la_derniere_modification();
            return;
        }
        // La caméra : « scanne cette facture », et, ouverte, « photo », « classe-la », « ferme ».
        if let Some(g) = parole::geste_photo(&t, self.camera) {
            self.geste_camera(g, t);
            return;
        }
        if parole::est_un_arret(&t) {
            self.ajouter("toi", t, Vec::new());
            if self.tours > 0 || self.parle || !self.en_attente.is_empty() || self.alerte_a_dire.is_some() {
                self.arreter();
            }
            return;
        }
        // Ce qu'Eve a coûté aujourd'hui : relu dans le journal, sa seule source, sans Claude.
        // Un rappel ou une minuterie dits : posés sans Claude (« rappelle-moi à 16 h d'appeler le plombier »).
        if let Some(r) = parole::rappel_dit(&t) {
            self.ajouter("toi", t.clone(), Vec::new());
            self.poser_un_rappel_dit(&t, r);
            return;
        }
        // Le point du jour : le tableau de bord, dit en une seconde, sans Claude.
        if parole::demande_le_point(&t) {
            self.ajouter("toi", t.clone(), Vec::new());
            let point = self.point_du_jour();
            if point.is_empty() {
                let rien = "Rien de prévu aujourd'hui, et aucune échéance qui presse.";
                self.repondre_sans_claude(&t, rien, rien);
            } else {
                let ecrit = point.iter().map(|l| format!("- {l}")).collect::<Vec<_>>().join("\n");
                self.repondre_sans_claude(
                    &t,
                    &format!("Le point :\n{ecrit}"),
                    &format!("Voici le point. {}", point.join(" ")),
                );
            }
            return;
        }
        if parole::demande_le_cout(&t) {
            self.ajouter("toi", t.clone(), Vec::new());
            let (demandes, cout) = mesures::bilan_du_jour(&journal::lire_tout(&dossier_local()), &journal::date());
            let (ecrit, dit) = if demandes == 0 && cout == 0.0 {
                let r = "Aujourd'hui, Claude ne m'a encore rien coûté.".to_string();
                (r.clone(), r)
            } else {
                let montant = mesures::dollars(cout);
                let s = if demandes > 1 { "s" } else { "" };
                (
                    format!(
                        "Aujourd'hui : environ **{montant}**, pour {demandes} demande{s}.\n---\nLe prix de Claude au tarif \
                         de l'API, d'après le journal ; avec un abonnement, c'en est l'équivalent."
                    ),
                    format!("Aujourd'hui, environ {}, pour {demandes} demande{s}.", montant.replace('$', "dollars")),
                )
            };
            self.repondre_sans_claude(&t, &ecrit, &dit);
            return;
        }
        let pieces = std::mem::take(&mut self.pieces);
        let noms: Vec<String> = pieces.iter().map(|p| p.nom.clone()).collect();
        self.ajouter("toi", t.clone(), noms.clone());
        if self.erreur.is_some() {
            let n = self.nommee();
            self.dire(&format!("Je ne peux pas : {} n'est pas démarré{}. Le détail est à l'écran.", n.le, n.e));
            return;
        }
        // L'heure de la demande, que Claude Code ne sait pas (il n'a que la date) : pour un rappel, « dans une heure ».
        let mut message = format!(
            "[Eve : nous sommes le {}, il est {}.]\n\n",
            crate::ambiance::date_longue(&journal::date()).to_lowercase(),
            rappels::heure_dite(&journal::date_et_heure(), &journal::date())
        );
        if std::mem::take(&mut self.memoire_a_donner) && !self.memoire.is_empty() {
            message.push_str(&memoire::pour_l_adjointe(&self.reglages.qui, &self.memoire));
            message.push_str("\n\n");
        }
        if let Some(n) = self.note_pour_l_adjointe.take() {
            message.push_str(&n);
            message.push_str("\n\n");
        }
        if let Some(r) = self.reprise.take() {
            message.push_str(&r);
            message.push_str("\n\n");
        }
        message.push_str(&t);
        if !pieces.is_empty() {
            message.push_str("\n\nPièces jointes à cette demande (déposées dans Eve, sur le poste) :");
            for p in &pieces {
                message.push_str(&format!("\n- {}", p.chemin.display()));
            }
        }
        if parole::demande_le_resume(&t) {
            message.push_str(&actions_du_jour());
        } else if parole::demande_les_nouvelles(&t) {
            // L'Adjointe a sa veille des courriels ; un spécialiste n'en a pas (il aurait lu « rien de neuf dans la veille
            // des courriels ») : il reçoit le point de son tableau de bord.
            if self.reglages.specialiste() {
                let point = self.point_du_jour();
                let point = if point.is_empty() {
                    "rien de prévu, aucune échéance qui presse".into()
                } else {
                    point.join(" ")
                };
                message.push_str(&format!("\n\n[Le tableau de bord d'Eve, à cette heure : {point}]"));
            } else {
                message.push_str(&self.releve_d_eve());
            }
            self.dernier_point = Some(journal::heure());
        } else if parole::parle_du_calendrier(&t) {
            message.push_str(&self.calendrier_pour_l_adjointe());
        }
        if self.tours > 0 && !self.prechauffage {
            self.dire("C'est noté ; je termine d'abord la demande en cours.");
        }
        self.en_attente.push_back(Message {
            texte: message,
            demande: Some(t),
            pieces: noms,
            au_chaud: false,
            a_la_voix,
            chrono: Some(Chrono::nouveau(fin_de_parole)),
        });
        self.suivant();
    }

    // ---- La caméra -------------------------------------------------------------------------------------------

    /// Un geste de la caméra, dit (ou écrit) : un ordre à la fenêtre, qui le joue une fois (son numéro).
    fn geste_camera(&mut self, g: GestePhoto, t: String) {
        journal::ecrire(&format!("caméra, geste « {} » : {t}", g.nom()));
        match g {
            GestePhoto::Ouvrir if !self.camera => {
                self.intentions_scan = vec![t];
                if self.avec_fenetre {
                    fenetre::montrer();
                }
                self.dire("Montre-moi le document.");
            }
            GestePhoto::Ouvrir => {
                if self.avec_fenetre {
                    fenetre::montrer();
                }
            }
            GestePhoto::Classer => self.intentions_scan.push(t),
            GestePhoto::Declic | GestePhoto::Reprendre | GestePhoto::Fermer => {}
        }
        self.camera_ordre = (self.camera_ordre.0 + 1, g.nom());
    }

    fn joindre(&mut self, nom: String, chemin: PathBuf, taille: u64) {
        self.compteur_pieces += 1;
        self.pieces.push(Piece { id: format!("p{}", self.compteur_pieces), nom, chemin, taille });
    }

    /// Une numérisation reçue : jointe à la prochaine demande, ou rangée dans `Scan\` — la boîte d'entrée de tout ce
    /// qui se numérise (CLAUDE.md, § Le dossier Scan) — et confiée à l'Adjointe pour qu'elle la classe (règle 7).
    fn scan(&mut self, nom: String, chemin: PathBuf, pages: usize, but: &str, sorte: &str, taille: u64) {
        journal::ecrire(&format!("numérisation ({but}, {pages} page(s), {sorte}) : {}", chemin.display()));
        let intentions: Vec<String> =
            std::mem::take(&mut self.intentions_scan).into_iter().filter(|i| parole::precise_le_geste(i)).collect();
        let s = if pages > 1 { "s" } else { "" };
        // Un poste qui a sa propre consigne n'a ni le `Scan\` ni la règle 7 de l'Adjointe : « classer » (« C'est bon »,
        // dit la caméra ouverte) y joint la numérisation à la prochaine demande.
        if but == "joindre" || self.propre {
            self.joindre(nom, chemin, taille);
            self.dire(&format!("C'est joint : {pages} page{s}. Pose ta question."));
            return;
        }
        // Une copie dans le Scan\ de l'entreprise (celui des réglages de l'Adjointe, sinon Scan\ à côté de son dossier) ; la
        // copie locale reste dans les pièces du jour.
        let scan = self
            .reglages
            .scan_lu
            .clone()
            .or_else(|| self.dossier.as_ref().and_then(|d| d.parent()).map(|g| g.join("Scan")));
        let dans_scan = scan.filter(|d| d.is_dir()).and_then(|d| {
            let cible = ecran::libre(&d, &nom);
            match std::fs::copy(&chemin, &cible) {
                Ok(_) => Some(cible),
                Err(e) => {
                    journal::ecrire(&format!("la numérisation n'a pas pu aller dans {} : {e}", d.display()));
                    None
                }
            }
        });
        let quoi =
            if sorte == "photo" { format!("{pages} photo{s}") } else { format!("un document de {pages} page{s}") };
        let mut texte = match &dans_scan {
            Some(c) => format!(
                "[Numérisation faite à l'instant par la caméra d'Eve, rangée dans Scan : {} — {quoi}.]",
                c.display()
            ),
            None => format!(
                "[Numérisation faite à l'instant par la caméra d'Eve, restée sur ce poste (le dossier Scan est introuvable) : \
                 {} — {quoi}.]",
                chemin.display()
            ),
        };
        for i in &intentions {
            texte.push_str(&format!("\nCe que la personne a dit : « {i} »"));
        }
        texte.push_str(
            "\nRegarde chaque page, dis-moi en une phrase ce que c'est, puis classe-le comme le veut la règle 7 (CLAUDE.md, \
             § Le dossier Scan) : au dossier du projet, à la bonne étape ; une facture reçue se prépare aussi pour la base \
             (compta_base.py inscrire, simulation d'abord) ; l'original quitte Scan une fois la copie vérifiée. Si le projet \
             ou la nature du document est douteux, demande-le-moi avant de ranger.",
        );
        let demande = format!("Classe ce que je viens de numériser ({pages} page{s})");
        self.ajouter("toi", demande.clone(), vec![nom.clone()]);
        if self.erreur.is_some() {
            let ou = if dans_scan.is_some() {
                "Le document attend dans Scan"
            } else {
                "Le document est gardé sur le poste"
            };
            self.dire(&format!("{ou} ; l'Adjointe n'est pas démarrée pour le classer."));
            return;
        }
        if self.tours > 0 && !self.prechauffage {
            self.dire("C'est noté ; je termine d'abord la demande en cours.");
        }
        let mut message = String::new();
        if let Some(r) = self.reprise.take() {
            message.push_str(&r);
            message.push_str("\n\n");
        }
        message.push_str(&texte);
        self.en_attente.push_back(Message {
            texte: message,
            demande: Some(demande),
            pieces: vec![nom],
            au_chaud: false,
            a_la_voix: false,
            chrono: Some(Chrono::nouveau(None)),
        });
        self.suivant();
    }

    // ---- L'historique ----------------------------------------------------------------------------------------

    /// Garde l'échange du tour qui s'achève (`issue` : « interrompue », « erreur », ou rien).
    fn historiser(&mut self, demande: &str, reponse: &str, issue: &str) {
        if !self.reglages.historique {
            return;
        }
        let e = Entree {
            conversation: self.conversation.clone(),
            date: journal::date(),
            heure: journal::heure(),
            demande: demande.to_string(),
            reponse: reponse.trim().to_string(),
            pieces: std::mem::take(&mut self.pieces_du_tour),
            issue: issue.to_string(),
        };
        if let Err(err) = historique::ajouter(&self.reglages.qui, &e) {
            journal::ecrire(&format!("historique : l'échange n'a pas pu s'écrire ({err})"));
        }
    }

    /// Reprend une conversation de l'historique : ses derniers échanges reviennent au fil, l'Adjointe les reçoit avec
    /// la demande suivante, et la suite s'y ajoute.
    fn reprendre_conversation(&mut self, id: &str) {
        let entrees = historique::conversation(&self.reglages.qui, id);
        let Some(premiere) = entrees.first() else {
            self.systeme("Cette conversation est introuvable dans l'historique.");
            return;
        };
        let n = entrees.len();
        self.ajouter(
            "systeme",
            format!("— Conversation du {} reprise ({n} échange{}) —", premiere.date, if n > 1 { "s" } else { "" }),
            Vec::new(),
        );
        let debut = n.saturating_sub(6);
        for e in &entrees[debut..] {
            self.fil.push(Ligne {
                de: "toi",
                texte: e.demande.clone(),
                heure: e.heure.clone(),
                pieces: e.pieces.clone(),
                ancien: true,
                tour: String::new(),
            });
            self.fil.push(Ligne {
                de: "eve",
                texte: e.reponse.clone(),
                heure: e.heure.clone(),
                pieces: Vec::new(),
                ancien: true,
                tour: String::new(),
            });
        }
        self.fil_rev += 1;
        if self.fil.len() > FIL_MAX {
            let trop = self.fil.len() - FIL_MAX;
            self.fil.drain(..trop);
        }
        self.reprise = Some(historique::pour_l_adjointe(&self.reglages.qui, &entrees));
        self.conversation = id.to_string();
        journal::ecrire(&format!("conversation reprise : {id} ({n} échanges)"));
        self.dire("C'est repris. Je t'écoute.");
    }

    /// Ce qu'Eve a vu de la journée, joint à « quoi de neuf ? » : la veille des courriels du poste, le calendrier.
    fn releve_d_eve(&self) -> String {
        let depuis = self.dernier_point.clone();
        let recentes: Vec<&Nouvelle> =
            self.nouvelles.iter().filter(|n| depuis.as_ref().is_none_or(|d| n.heure.as_str() > d.as_str())).collect();
        let quand = match &depuis {
            Some(d) => format!("depuis {d}"),
            None => "aujourd'hui".to_string(),
        };
        let mut s =
            format!("\n\n[Ce qu'Eve a vu {quand} — la veille des courriels de ce poste et le calendrier du jour :");
        if recentes.is_empty() {
            s.push_str("\n- rien de neuf dans la veille des courriels");
        }
        let debut = recentes.len().saturating_sub(25);
        for n in &recentes[debut..] {
            let marque = if n.sorte == Sorte::Suspect { "SUSPECT · " } else { "" };
            s.push_str(&format!("\n- {} · {marque}{}", n.heure, n.texte));
        }
        if self.agenda.is_empty() {
            if self.jeton.as_ref().is_some_and(Option::is_some) {
                s.push_str("\nCalendrier du jour : pas encore relu.");
            }
        } else {
            s.push_str("\nAujourd'hui au calendrier :");
            for r in &self.agenda {
                let heure = if r.heure.is_empty() { String::new() } else { format!("{} · ", r.heure) };
                let detail = if r.detail.is_empty() { String::new() } else { format!(" ({})", r.detail) };
                s.push_str(&format!("\n- {heure}{}{detail}", r.titre));
            }
        }
        s.push(']');
        s
    }

    /// Les quinze jours du calendrier, joints à une question de calendrier : l'Adjointe y lit sa réponse sans relire la
    /// base — un aller-retour de moins (trois secondes, mesuré le 2026-10-22).
    fn calendrier_pour_l_adjointe(&self) -> String {
        bloc_calendrier(&self.calendrier_lu, &self.calendrier)
    }

    // ---- L'Adjointe --------------------------------------------------------------------------------------------

    /// Un tour commence : celui d'une demande ([`Self::suivant`]), ou celui que Claude ouvre de lui-même quand un de ses
    /// agents a fini ([`Self::flux_recu`]). Eve n'envoie rien d'autre avant sa fin.
    fn commencer_un_tour(&mut self, maintenant: Instant) {
        self.tours = 1;
        self.interrompu = false;
        self.dernier_tour = maintenant;
        self.debut_tour = Some(maintenant);
        self.dernier_signe = maintenant;
        self.annonce = false;
        self.rassure = 0;
        self.dit_dans_le_tour = false;
        self.commentaires.clear();
        self.en_cours.clear();
        self.flux.nouveau_tour();
        // Le visage quitte l'émotion de la réponse d'avant ; la nouvelle viendra en tête de celle-ci.
        self.oublier_les_tons();
        self.emotion = None;
    }

    /// Les émotions de la réponse s'oublient (une autre commence) : celles qu'on retenait, ses passages.
    fn oublier_les_tons(&mut self) {
        (self.tons, self.passages) = (parole::Tons::default(), Passages::default());
    }

    /// Le visage prend l'émotion d'un passage, s'il y en a une.
    fn montrer(&mut self, e: Option<parole::Emotion>) {
        if let Some(e) = e {
            self.emotion = Some((e, Instant::now()));
        }
    }

    /// Envoie le message suivant si l'Adjointe est libre : un seul à la fois.
    fn suivant(&mut self) {
        // Un profil ou un cerveau choisis d'abord : ce qui attend partira avec eux ([`Self::suivre_claude`]) ; une
        // relance dite : Eve se ferme dans un instant, aucun tour ne commence (sa demande attendrait pour rien).
        if !self.init_ok
            || self.tours > 0
            || self.cerveau.is_none()
            || self.claude_a_suivre
            || self.relance_dite.is_some()
        {
            return;
        }
        // Les outils de Constructo AI arrivent (le branchement, ou le renouvellement qui le précède) : une demande de la
        // personne les attend, sans quoi elle partirait sans eux ; un tour silencieux, non.
        if self.outils_en_route() && self.en_attente.front().is_some_and(|m| m.demande.is_some()) {
            return;
        }
        let Some(m) = self.en_attente.pop_front() else { return };
        self.prechauffage = m.demande.is_none();
        self.au_chaud = m.au_chaud;
        self.demande_du_tour = m.demande;
        self.tour_a_la_voix = m.a_la_voix;
        self.tour_numero += 1;
        self.pieces_du_tour = m.pieces;
        let maintenant = Instant::now();
        self.commencer_un_tour(maintenant);
        self.chrono = m.chrono.map(|mut c| {
            c.envoyee = Some(maintenant);
            c
        });
        if let Some(Err(e)) = self.cerveau.as_ref().map(|c| c.dire(&m.texte)) {
            self.tours = 0;
            self.prechauffage = false;
            self.chrono = None;
            self.systeme(&format!("{} ne répond pas : {e}", self.nommee().le_maj));
        }
    }

    /// La garde est branchée (et la base aussi : Claude Code l'attend avant le premier tour, `cerveau.rs`) : le
    /// préchauffage, puis ce qui attendait.
    fn base_prete(&mut self) {
        if self.reglages.prechauffage && self.en_attente.is_empty() && !self.lance_en_reprise {
            let texte = self.message_de_prechauffage();
            self.en_attente.push_front(Message::silencieux(texte, false));
        }
        self.suivant();
    }

    fn message_de_prechauffage(&mut self) -> String {
        let mut m = String::from(
            "[Eve démarre — préchauffage silencieux. Ne lance aucun outil et ne lis aucun fichier ; réponds seulement \
             « Prête. »]",
        );
        if std::mem::take(&mut self.memoire_a_donner) && !self.memoire.is_empty() {
            m.push_str("\n\n");
            m.push_str(&memoire::pour_l_adjointe(&self.reglages.qui, &self.memoire));
        }
        m
    }

    fn claude(&mut self, v: &Value) {
        self.dernier_signe = Instant::now();
        match protocole::lire(v) {
            Recu::Ouverture { session, modele, serveurs, outils } => {
                journal::ecrire(&format!(
                    "Claude Code ouvert : session {session}, modèle {modele}, serveurs {serveurs:?}"
                ));
                if !std::mem::replace(&mut self.outils_vus, true) {
                    let hors: Vec<&str> =
                        outils.iter().map(String::as_str).filter(|o| self.espace.refus(o).is_some()).collect();
                    if !hors.is_empty() {
                        journal::ecrire(&format!(
                            "Claude Code offre {} outil(s) hors des privilèges ; la garde les refuse : {}",
                            hors.len(),
                            garde::ecourter(&hors.join(", "), 600)
                        ));
                    }
                }
                if self.session.as_deref() != Some(session.as_str()) || self.seance_du.is_none() {
                    // Une séance neuve commence aujourd'hui ; une séance reprise garde son jour.
                    if !self.lance_en_reprise || self.seance_du.is_none() {
                        self.seance_du = Some(journal::date());
                    }
                }
                self.session = Some(session);
                self.noter_la_seance();
                if self.jeton.as_ref().is_some_and(Option::is_some) {
                    let en_connexion = serveurs.iter().any(|(n, s)| n == "entrepreneur-general" && s == "pending");
                    if self.prechauffage && en_connexion {
                        self.rechauffer = true;
                    }
                    self.base = match serveurs.iter().find(|(n, _)| n == "entrepreneur-general") {
                        Some((_, s)) if s == "connected" => ("connectee", "entrepreneur-general.ca".into()),
                        // Encore en connexion à l'ouverture : le premier outil de la base qui répond met le voyant au vert.
                        Some((_, s)) if s == "pending" => ("inconnue", "connexion à la base…".into()),
                        Some((_, s)) => ("absente", format!("serveur de la base : {s}")),
                        None => ("absente", "serveur de la base non branché".into()),
                    };
                }
            }
            Recu::Flux(f) => self.flux_recu(f),
            Recu::Adjointe { outils, panne, .. } => {
                if !panne.is_empty() {
                    journal::ecrire(&format!("Claude rend une erreur de l'API : {panne}"));
                    self.panne = panne;
                }
                for (id, nom, entree) in outils {
                    let resume = protocole::resume_outil(&nom, entree);
                    journal::ecrire(&format!("outil {resume}"));
                    self.activite.push_back(resume);
                    while self.activite.len() > 6 {
                        self.activite.pop_front();
                    }
                    self.commenter(&nom, entree);
                    self.noter_outil(&id, &nom, entree, false, false);
                    self.en_route.insert(id, (nom, entree.clone()));
                }
            }
            Recu::Resultats { outils } => {
                for (id, en_erreur) in outils {
                    let Some((nom, entree)) = self.en_route.remove(&id) else { continue };
                    self.noter_outil(&id, &nom, &entree, true, en_erreur);
                    if nom.starts_with("mcp__entrepreneur-general__") && !en_erreur {
                        self.base = ("connectee", "entrepreneur-general.ca".into());
                    }
                    // Un outil de la connexion d'Eve qui répond : Constructo AI est bien là.
                    if !en_erreur && nom.starts_with(&format!("mcp__{}__", garde::SERVEUR_CONSTRUCTO)) {
                        self.constructo_branche = true;
                        self.constructo_erreur = None;
                    }
                    if !en_erreur {
                        self.noter_action(&nom, &entree);
                    }
                    let vu = self.vus.remove(&id) || self.retirer_signature(&signature(&nom, &entree));
                    if vu || self.decision(&nom, &entree) == Decision::Libre {
                        continue;
                    }
                    if en_erreur {
                        // Rejeté par Claude Code avant le crochet (une entrée invalide, un outil annulé) : rien n'a tourné.
                        journal::ecrire(&format!("outil rejeté avant la garde (rien n'a tourné) : {nom} ({id})"));
                        continue;
                    }
                    // Un outil qui demandait l'accord a tourné sans que la garde le voie : on arrête tout.
                    journal::ecrire(&format!("GARDE CONTOURNÉE : {nom} ({id})"));
                    let n = self.nommee();
                    self.erreur = Some(format!(
                        "La garde n'a pas vu passer « {nom} » : Eve arrête {} par prudence. \
                         « Nouvelle conversation » {} relance ; si cela revient, il faut le signaler.",
                        n.le, n.la
                    ));
                    if let Some(c) = self.cerveau.take() {
                        c.tuer();
                    }
                    self.couper();
                    self.dire(&format!("J'ai dû arrêter {} par prudence. Le détail est à l'écran.", n.le));
                }
            }
            Recu::Fin { texte, erreur, cout, duree_ms } => self.fin_du_tour(texte, erreur, cout, duree_ms),
            Recu::Demande { id, sorte, corps } => {
                let corps = corps.clone();
                self.demande(&id, &sorte, &corps);
            }
            Recu::Reponse { id, reussie, erreur, corps } => {
                if id == self.id_init {
                    self.init_attendue = None;
                    if reussie {
                        self.init_ok = true;
                        journal::ecrire("garde branchée (initialisation acceptée)");
                        // Constructo AI d'abord : les demandes de la personne l'attendront, le préchauffage non.
                        self.brancher_constructo();
                        self.base_prete();
                    } else {
                        let n = self.nommee();
                        self.echec_grave(format!(
                            "La garde n'a pas pu se brancher ({erreur}) : Eve n'ouvre pas {} sans elle.",
                            n.le
                        ));
                        if let Some(c) = self.cerveau.take() {
                            c.tuer();
                        }
                    }
                } else if self.branchement.as_ref().is_some_and(|b| b.id == id) {
                    let corps = corps.clone();
                    self.branchement_fait(reussie, &erreur, &corps);
                } else if !reussie {
                    journal::ecrire(&format!(
                        "demande {id} refusée par Claude Code : {}",
                        connexion::sans_porteur(&erreur)
                    ));
                }
            }
            Recu::Abandon { id } => {
                let tete = self.attentes.front().is_some_and(|a| a.id == id);
                self.attentes.retain(|a| a.id != id);
                if tete {
                    self.systeme(&format!("{} a retiré sa demande d'accord.", self.nommee().le_maj));
                    self.tete_depuis = Instant::now();
                    self.annoncer_accord();
                }
            }
            Recu::Agents(agents) => self.suivre_les_agents(agents),
            Recu::Limite { etat, reprise, sorte, part } => self.suivre_la_limite(&etat, reprise, &sorte, part),
            Recu::Reessai { tentative, permis, panne, sans_reponse } => {
                journal::ecrire(&format!(
                    "Claude Code réessaie l'API ({tentative}/{permis}) : {panne}{}",
                    if sans_reponse { ", aucune réponse" } else { "" }
                ));
                let panne = if sans_reponse { "reseau".to_string() } else { panne };
                // À la deuxième tentative, Eve le dit une fois : sans cela, un long silence.
                if tentative >= 2 && !self.reessai_dit {
                    self.reessai_dit = true;
                    self.dire(match panne.as_str() {
                        "reseau" => "Claude ne répond pas ; je réessaie.",
                        "overloaded" => "Claude est surchargé ; je réessaie.",
                        "rate_limit" => "Claude me fait attendre ; je réessaie.",
                        _ => "Claude a un souci ; je réessaie.",
                    });
                }
                self.panne = panne;
            }
            Recu::Autre => {}
        }
    }

    /// Les agents de Claude en arrière-plan ont changé (un qui part, un qui finit) : le journal le dit, et un autre
    /// profil, un autre cerveau ou une mise à jour attendent qu'ils aient tous fini.
    fn suivre_les_agents(&mut self, agents: Vec<String>) {
        // Un agent qui n'y est plus a fini : Claude va en rendre compte, dans un tour qu'il ouvre de lui-même.
        if self.agents.iter().any(|a| !agents.contains(a)) {
            self.compte_rendu_attendu = Some(Instant::now());
        }
        if agents.len() != self.agents.len() {
            journal::ecrire(&if agents.is_empty() {
                "agents : tous ont fini".to_string()
            } else {
                format!("agents au travail : {} ({})", agents.len(), garde::ecourter(&agents.join(" ; "), 300))
            });
        }
        self.agents = agents;
    }

    /// Un outil de Claude commence ou finit : un événement pour le réseau des actions (les 60 derniers).
    fn noter_outil(&mut self, id: &str, nom: &str, entree: &Value, fin: bool, erreur: bool) {
        self.outils_n += 1;
        self.outils.push_back(json!({
            "n": self.outils_n, "id": id, "outil": nom, "detail": protocole::detail_outil(nom, entree),
            "cible": protocole::cible_outil(nom, entree), "fin": fin, "erreur": erreur,
        }));
        while self.outils.len() > 60 {
            self.outils.pop_front();
        }
    }

    /// La réponse en train de s'écrire : affichée au fil des mots, dite phrase par phrase.
    fn flux_recu(&mut self, f: Flux) {
        if self.prechauffage || self.interrompu {
            return;
        }
        match f {
            Flux::Debut => {
                // Claude parle sans qu'Eve lui ait rien envoyé : un de ses agents a fini, et il en rend compte. C'est un
                // tour, qu'aucune demande ne coupe ; sa réponse se dit comme une autre.
                if self.tours == 0 {
                    journal::ecrire("Claude reprend la parole de lui-même (un agent a fini)");
                    self.compte_rendu_attendu = None;
                    self.demande_du_tour = None;
                    self.chrono = None;
                    self.commencer_un_tour(Instant::now());
                }
                // Ce que le message d'avant retenait encore, avant que celui-ci ne commence (une émotion peut l'ouvrir).
                self.vider_les_tons();
                self.flux.nouveau_message();
                if !self.en_cours.trim().is_empty() && !self.en_cours.ends_with("\n\n") {
                    self.en_cours.push_str("\n\n");
                }
                self.tons = parole::Tons::default();
            }
            Flux::Texte(t) => {
                // Les émotions (« [triste] » en tête, « [serieuse] » où le ton change) s'ôtent : elles ne se disent ni ne
                // s'affichent ; le visage les prend quand leur passage se dit.
                for m in self.tons.ajouter(&t) {
                    self.morceau(m);
                }
            }
            Flux::FinBloc => {
                self.vider_les_tons();
                for p in self.flux.finir() {
                    self.dire_la_reponse(&p);
                }
            }
            Flux::DebutTexte | Flux::Outil(_) => {}
        }
    }

    /// Un morceau de la réponse : du texte, affiché et dit ; ou l'émotion du passage qui commence — ce qui la précède se
    /// dit avec le ton d'avant ([`EnFlux::couper`]).
    fn morceau(&mut self, m: parole::Morceau) {
        match m {
            parole::Morceau::Texte(t) => self.texte_de_la_reponse(&t),
            parole::Morceau::Emotion(e) => {
                for p in self.flux.couper() {
                    self.dire_la_reponse(&p);
                }
                self.passages.changer(e);
            }
        }
    }

    /// Un morceau de la réponse : affiché au fil des mots, dit phrase par phrase ; le premier mot chronométré.
    fn texte_de_la_reponse(&mut self, t: &str) {
        if let Some(c) = &mut self.chrono
            && c.premier_mot.is_none()
            && !t.trim().is_empty()
        {
            c.premier_mot = Some(Instant::now());
        }
        self.en_cours.push_str(t);
        for p in self.flux.ajouter(t) {
            self.dire_la_reponse(&p);
        }
    }

    /// Ce que retenaient les émotions à la fin d'un bloc : une émotion seule au bout, un « [ » resté ouvert (du texte).
    fn vider_les_tons(&mut self) {
        for m in self.tons.finir() {
            self.morceau(m);
        }
    }

    /// Ce que fait l'Adjointe, dit en quelques mots quand rien d'autre ne se dit.
    fn commenter(&mut self, nom: &str, entree: &Value) {
        if self.prechauffage
            || self.interrompu
            || !self.attentes.is_empty()
            || self.parle
            || self.commentaires.len() >= 2
        {
            return;
        }
        if self.dernier_dit.is_some_and(|d| d.elapsed() < Duration::from_secs(4)) {
            return;
        }
        let Some((sorte, phrase)) = commentaire(nom, entree) else { return };
        if !self.commentaires.insert(sorte) {
            return;
        }
        self.annonce = true;
        self.dire_phrase(phrase);
    }

    fn fin_du_tour(&mut self, texte: String, erreur: bool, cout: Option<f64>, duree_ms: Option<u64>) {
        // Le compte rendu d'un agent qui a fini attend la fin du tour en cours : son attente repart d'ici.
        if let Some(depuis) = &mut self.compte_rendu_attendu {
            *depuis = Instant::now();
        }
        self.tours = 0;
        self.au_chaud = false;
        self.dernier_tour = Instant::now();
        self.activite.clear();
        self.en_route.clear();
        // Claude Code rend le cumul de sa séance : le coût du tour en est la différence. Dans une même séance (le cumul
        // repart à chaque lancement de Claude Code), il ne baisse jamais : un cumul plus bas — un tour en erreur rend
        // parfois 0 $ — ne compte pour rien, et ne remet pas le compte à zéro (le tour suivant aurait compté toute la
        // séance). Au dix-millième : un tour de Haiku coûte moins d'un dixième de cent.
        let du_tour = cout.map(|c| match self.cumul {
            Some(avant) if c < avant => 0.0,
            avant => {
                self.cumul = Some(c);
                mesures::cout_du_tour(avant, c)
            }
        });
        let secondes = duree_ms.unwrap_or(0) as f64 / 1000.0;
        let cumul = self.cumul.unwrap_or_default();
        journal::ecrire(&format!(
            "fin du tour ({secondes:.1} s, {}) : {}",
            du_tour.map(|t| format!("{t:.4} $ ; cumul {cumul:.4} $")).unwrap_or_default(),
            garde::ecourter(&texte, 400)
        ));
        // Ce que la réponse a pris, montré sous elle : sa durée, son coût, le cerveau qui l'a faite.
        let cerveau = format!(
            "{}, {}",
            crate::reglages::nom_du_modele(&self.reglages.modele),
            crate::reglages::nom_de_l_effort(&self.reglages.effort)
        );
        let tour = match du_tour {
            Some(t) => format!("{} · {} · {cerveau}", mesures::duree(secondes), mesures::dollars(t)),
            None => format!("{} · {cerveau}", mesures::duree(secondes)),
        };
        if std::mem::take(&mut self.prechauffage) {
            self.en_cours.clear();
            self.flux.nouveau_tour();
            self.oublier_les_tons();
            self.emotion = None;
            // Un préchauffage en erreur : la personne le sait avant sa première demande (un compte déconnecté, une
            // limite atteinte), au lieu d'une Eve « prête » qui échouera.
            if erreur {
                journal::ecrire(&format!("le préchauffage a échoué : {}", garde::ecourter(&texte, 300)));
                match self.phrase_de_la_panne() {
                    Some((ecrit, dit)) => {
                        self.systeme(&ecrit);
                        self.dire_a_voix_haute(&dit);
                    }
                    None => self
                        .systeme(&format!("{} ne répond pas comme il faut : voir le journal.", self.nommee().le_maj)),
                }
            }
            if std::mem::take(&mut self.rechauffer) && self.rechauffages < 2 {
                // La base s'est branchée pendant ce tour : un second, silencieux, met le bon contexte en cache.
                self.rechauffages += 1;
                journal::ecrire("la base s'est branchée pendant le préchauffage : un second tour silencieux suit");
                let texte = "[Eve : la base est branchée — tour silencieux. Ne lance aucun outil ; réponds seulement « Prête. »]";
                self.en_attente.push_front(Message::silencieux(texte, true));
            } else {
                let n = self.nommee();
                journal::ecrire(&format!("préchauffage fait : {} est prêt{}", n.le, n.e));
            }
            self.suivant();
            return;
        }
        let interrompu = std::mem::take(&mut self.interrompu);
        if !interrompu {
            self.vider_les_tons();
            for p in self.flux.finir() {
                self.dire_la_reponse(&p);
            }
        }
        // La réponse entière, sans ses émotions (sans le flux, c'est ici qu'elle se lit). Rien n'en a été dit — elle ne
        // se lit qu'à l'écran, ou d'un coup — et le visage n'a rien pris : l'émotion de tête, tout de suite.
        let (emotions, texte_seul) = parole::sans_emotions(&texte);
        if !interrompu && self.emotion.is_none() && self.passages.vide() {
            self.montrer(self.passages.premier().or(emotions.first().copied()));
        }
        self.tons = parole::Tons::default();
        let a_parle = self.flux.a_parle();
        let ecrit = std::mem::take(&mut self.en_cours);
        let affiche = if ecrit.trim().is_empty() { texte_seul.clone() } else { ecrit };
        let demande = self.demande_du_tour.take();
        if interrompu {
            // Affiché, pas dit : la personne vient de dire « arrête ».
            if let Some(d) = &demande {
                self.historiser(d, &affiche, "interrompue");
            }
            if !affiche.trim().is_empty() {
                self.ajouter("eve", affiche, Vec::new());
            }
        } else if erreur {
            let t = if affiche.trim().is_empty() { "Le tour s'est mal terminé.".to_string() } else { affiche };
            if let Some(d) = &demande {
                self.historiser(d, &t, "erreur");
            }
            self.ajouter("eve", t, Vec::new());
            // La panne dite pour ce qu'elle est (la limite, le compte, le réseau…), sinon le message d'office.
            match self.phrase_de_la_panne() {
                Some((ecrit, dit)) => {
                    self.systeme(&ecrit);
                    self.dire_a_voix_haute(&dit);
                }
                None => self.dire("Il y a eu un problème. Le détail est à l'écran."),
            }
        } else {
            if let Some(d) = &demande {
                self.historiser(d, &affiche, "");
            }
            // Un tour vide (Claude Code en rend un quand un agent a fini sans qu'il ait rien à dire) : pas de bulle vide.
            if !affiche.trim().is_empty() {
                self.ajouter("eve", affiche.clone(), Vec::new());
                if let Some(l) = self.fil.last_mut() {
                    l.tour = tour;
                }
                // Ses pages pour la voix : la première vient de se dire (en sourdine, « répète » la dira).
                self.pages = parole::pages_pour_la_voix(&affiche);
                self.page = 1;
                // Les rappels que Claude a posés : ses cartes « rappel », dans rappels.json.
                for json in crate::natif::cartes_de(&affiche) {
                    self.poser_un_rappel_de_claude(&json);
                }
            }
            if !a_parle {
                // Sans la réponse en flux (une ancienne version de Claude Code) : la réponse entière, d'un coup.
                let dit = parole::pour_la_voix(&texte_seul);
                if !dit.is_empty() {
                    let parole_id = self.dire_a_voix_haute(&dit);
                    self.premiere_phrase(parole_id);
                }
            }
            if let Some(d) = demande {
                self.memoriser(d, &affiche);
            }
        }
        self.ranger_le_chrono();
        self.flux.nouveau_tour();
        // Le dernier tour de la séance, au fichier : un redémarrage proche la reprendra.
        self.noter_la_seance();
        self.suivant();
    }

    /// Une phrase de la réponse : dite, et la première de la demande, chronométrée ; l'émotion de son passage, que le
    /// visage prend tout de suite, ou quand la phrase d'avant aura fini ([`Passages::phrase`]).
    fn dire_la_reponse(&mut self, phrase: &str) {
        let parole_id = self.dire_phrase(phrase);
        self.premiere_phrase(parole_id);
        let e = self.passages.phrase(parole_id);
        self.montrer(e);
    }

    /// La première phrase de la réponse est donnée à la voix (`parole_id` : son numéro de parole ; aucun en sourdine).
    fn premiere_phrase(&mut self, parole_id: Option<u64>) {
        if let Some(c) = &mut self.chrono
            && c.premiere_phrase.is_none()
        {
            c.premiere_phrase = Some(Instant::now());
            c.parole_id = parole_id;
        }
    }

    /// Le tour est fini : ses mesures se notent, ou attendent que sa première phrase s'entende.
    fn ranger_le_chrono(&mut self) {
        let Some(c) = self.chrono.take() else { return };
        if !c.attend_la_voix() {
            noter_les_mesures(&c);
        } else if let Some(avant) = self.chrono_a_entendre.replace(c) {
            noter_les_mesures(&avant);
        }
    }

    /// La voix avance : une parole s'entend (`entendue`), ou finit — dite, coupée ou en échec. Une première phrase qui
    /// a fini sans s'entendre ne s'entendra plus ; les mesures d'un tour fini se notent dès qu'elles sont complètes.
    fn suivre_la_voix(&mut self, id: u64, entendue: bool) {
        let maintenant = Instant::now();
        for c in [self.chrono.as_mut(), self.chrono_a_entendre.as_mut()].into_iter().flatten() {
            match c.parole_id {
                Some(p) if entendue && p == id && c.entendue.is_none() => c.entendue = Some(maintenant),
                Some(p) if !entendue && p <= id && c.entendue.is_none() => c.parole_id = None,
                _ => {}
            }
        }
        if self.chrono_a_entendre.as_ref().is_some_and(|c| !c.attend_la_voix())
            && let Some(c) = self.chrono_a_entendre.take()
        {
            noter_les_mesures(&c);
        }
    }

    fn memoriser(&mut self, demande: String, reponse: &str) {
        if !self.reglages.memoire {
            return;
        }
        let e = Echange {
            date: journal::date(),
            heure: journal::heure(),
            demande: garde::ecourter(&demande, 300),
            reponse: garde::ecourter(sans_cartes(reponse).trim(), 700),
        };
        memoire::ajouter(&self.reglages.qui, e.clone());
        self.memoire.push(e);
    }

    /// Les trois derniers échanges d'une séance précédente, plus pâles, en tête du fil (la page les annonce :
    /// « Séance précédente »).
    fn montrer_la_memoire(&mut self) {
        let debut = self.memoire.len().saturating_sub(3);
        let echanges: Vec<Echange> = self.memoire[debut..].to_vec();
        for e in echanges {
            self.fil.push(Ligne {
                de: "toi",
                texte: e.demande,
                heure: e.heure.clone(),
                pieces: Vec::new(),
                ancien: true,
                tour: String::new(),
            });
            self.fil.push(Ligne {
                de: "eve",
                texte: e.reponse,
                heure: e.heure,
                pieces: Vec::new(),
                ancien: true,
                tour: String::new(),
            });
        }
        self.fil_rev += 1;
    }

    /// Une demande de Claude Code : le crochet de la garde (avant chaque outil) ou une permission.
    fn demande(&mut self, id: &str, sorte: &str, corps: &Value) {
        let (nom, entree, outil_id, sorte) = match sorte {
            "hook_callback" => (
                corps.pointer("/input/tool_name"),
                corps.pointer("/input/tool_input"),
                corps.get("tool_use_id").or_else(|| corps.pointer("/input/tool_use_id")),
                Sorte2::Crochet,
            ),
            "can_use_tool" => {
                (corps.get("tool_name"), corps.get("input"), corps.get("tool_use_id"), Sorte2::Permission)
            }
            _ => {
                self.repondre_erreur(id, "Eve ne prend pas en charge cette demande.");
                return;
            }
        };
        let nom = nom.and_then(Value::as_str).unwrap_or("").to_string();
        let entree = entree.cloned().unwrap_or(Value::Null);
        if let Some(t) = outil_id.and_then(Value::as_str) {
            self.vus.insert(t.to_string());
        }
        *self.signatures.entry(signature(&nom, &entree)).or_insert(0) += 1;
        match self.decision(&nom, &entree) {
            Decision::Libre => {
                // Sans accords (le privilège retiré) : ce qui les aurait demandés part, et le journal le note, pour qu'on
                // puisse relire ce qui s'est fait.
                if !self.espace.accords
                    && let Some(Decision::Confirmer { quoi, detail }) =
                        garde::accord_voulu(&nom, &entree, self.tour_a_la_voix)
                {
                    journal::ecrire(&format!(
                        "sans accord (le privilège « accords » est retiré) : {quoi} — {}",
                        garde::ecourter(&detail, 300)
                    ));
                }
                self.garder_une_copie(&nom, &entree);
                let corps = match sorte {
                    Sorte2::Crochet => protocole::crochet_neutre(),
                    Sorte2::Permission => protocole::permission_accord(&entree),
                };
                self.repondre(id, corps);
            }
            Decision::Refuser { raison } => {
                journal::ecrire(&format!("refusé d'office : {nom} — {raison}"));
                self.systeme(&format!("✗ Refusé d'office : {raison}"));
                let corps = match sorte {
                    Sorte2::Crochet => protocole::crochet_refus(&raison),
                    Sorte2::Permission => protocole::permission_refus(&raison),
                };
                self.repondre(id, corps);
            }
            Decision::Confirmer { quoi, detail } => {
                journal::ecrire(&format!("accord demandé : {quoi} — {}", garde::ecourter(&detail, 300)));
                let premiere = self.attentes.is_empty();
                self.numero_accord += 1;
                self.attentes.push_back(Attente {
                    numero: self.numero_accord,
                    id: id.to_string(),
                    sorte,
                    quoi,
                    detail,
                    entree,
                    depuis: Instant::now(),
                });
                if premiere {
                    self.tete_depuis = Instant::now();
                    self.annoncer_accord();
                }
            }
        }
    }

    fn retirer_signature(&mut self, s: &str) -> bool {
        match self.signatures.get_mut(s) {
            Some(n) if *n > 1 => {
                *n -= 1;
                true
            }
            Some(_) => {
                self.signatures.remove(s);
                true
            }
            None => false,
        }
    }

    fn annoncer_accord(&mut self) {
        if let Some(q) = self.attentes.front().map(|a| a.quoi.clone()) {
            self.dire(&format!("J'ai besoin de ton accord pour {q}. Dis « je confirme », ou « annule »."));
        }
    }

    /// La réponse de la personne à la demande d'accord en tête.
    fn decider(&mut self, oui: bool) {
        let Some(a) = self.attentes.pop_front() else { return };
        self.repondre(&a.id, corps_de_decision(&a, oui));
        journal::ecrire(&format!("{} : {}", if oui { "accordé" } else { "refusé" }, a.quoi));
        self.systeme(&format!("{} {}", if oui { "✓ Accordé :" } else { "✗ Refusé :" }, a.quoi));
        self.tete_depuis = Instant::now();
        if self.attentes.is_empty() {
            self.dire(if oui { "C'est parti." } else { "D'accord, je n'écris rien." });
        } else {
            self.annoncer_accord();
        }
    }

    fn arreter(&mut self) {
        self.couper();
        while let Some(a) = self.attentes.pop_front() {
            self.repondre(&a.id, corps_de_decision(&a, false));
        }
        let retires = self.en_attente.iter().filter(|m| m.demande.is_some()).count();
        self.en_attente.retain(|m| m.demande.is_none());
        self.alertes.clear();
        self.alerte_a_dire = None;
        self.ecoute_suite = None;
        if self.tours > 0
            && let Some(c) = &self.cerveau
            && c.interrompre().is_ok()
        {
            self.interrompu = true;
        }
        self.systeme(if retires > 0 { "Interrompu (les demandes en attente sont retirées)." } else { "Interrompu." });
    }

    fn repondre(&mut self, id: &str, corps: Value) {
        if let Some(Err(e)) = self.cerveau.as_ref().map(|c| c.repondre(id, corps)) {
            journal::ecrire(&format!("réponse à {id} impossible : {e}"));
        }
    }

    fn repondre_erreur(&mut self, id: &str, erreur: &str) {
        if let Some(Err(e)) = self.cerveau.as_ref().map(|c| c.repondre_erreur(id, erreur)) {
            journal::ecrire(&format!("réponse à {id} impossible : {e}"));
        }
    }

    fn lancer_cerveau(&mut self) {
        // Le dossier du profil a disparu, ou son CLAUDE.md (une relance, une nouvelle conversation) : Claude Code ne
        // peut pas y travailler. Eve le dit et revient à son profil d'office — sa conversation, si ce lancement en
        // reprenait une ; une conversation neuve s'il en voulait une. reglages.json garde le choix : le dossier
        // reviendra peut-être.
        if let Some((nom, manque)) =
            self.profil.as_ref().and_then(|p| Some((p.nom.clone(), profils::manque(&p.dossier)?)))
        {
            let m = format!(
                "Le profil {nom} ne peut plus servir : {manque}. Je reviens à mon profil d'office, {}.",
                nom_affiche()
            );
            journal::ecrire(&format!("profil : {m}"));
            self.systeme(&m);
            let neuve = !self.reprendre;
            self.echanger_les_profils(None);
            if neuve {
                self.session = None;
                self.reprendre = false;
                self.conversation = journal::horodatage();
            }
        }
        let (Some(dossier), Some(claude), Some(jeton)) =
            (self.dossier_de_claude(), self.claude.clone(), self.jeton.clone())
        else {
            return;
        };
        let reprendre = if std::mem::take(&mut self.reprendre) { self.session.clone() } else { None };
        self.lance_en_reprise = reprendre.is_some();
        let l = Lancement {
            claude,
            dossier_d_eve: self.dossier.clone().unwrap_or_else(|| dossier.clone()),
            dossier,
            profil: self.profil.as_ref().map(|p| p.nom.clone()).unwrap_or_default(),
            modele: self.reglages.modele.clone(),
            effort: self.reglages.effort.clone(),
            qui: self.reglages.qui.clone(),
            entreprise: self.reglages.nom_entreprise().to_string(),
            jeton_mcp: jeton,
            reprendre,
            generation: self.generation,
            espace: self.espace.clone(),
        };
        match Cerveau::lancer(&l, self.rapport.clone()) {
            Ok(c) => {
                self.lance_avec = (l.modele.clone(), l.effort.clone());
                journal::ecrire(&format!(
                    "Claude Code lancé ({}, effort {}{}) dans {}",
                    l.modele,
                    l.effort,
                    if self.lance_en_reprise { ", la conversation reprise" } else { "" },
                    l.dossier.display()
                ));
                journal::ecrire(&format!("privilèges de Claude : {}", self.espace.resume()));
                if self.espace.constructo {
                    let relie = &self.connexion;
                    journal::ecrire(&match (relie.connectee(), relie.compte.as_str()) {
                        (true, "") => "Constructo AI : relié, branché après l'initialisation".to_string(),
                        (true, compte) => {
                            format!("Constructo AI : relié au compte {compte}, branché après l'initialisation")
                        }
                        (false, _) => "Constructo AI : pas encore relié (« Se connecter à Constructo AI »)".to_string(),
                    });
                }
                self.id_init = c.id_init.clone();
                self.cerveau = Some(c);
                self.init_attendue = Some(Instant::now());
                self.cumul = None;
                self.outils_vus = false;
                // Une séance neuve n'a aucun serveur d'Eve : Constructo AI s'y rebranche après l'initialisation.
                self.branchement = None;
                self.constructo_branche = false;
            }
            Err(e) => self.echec_grave(format!("Claude Code ne démarre pas : {e}")),
        }
    }

    /// Une panne qui arrête l'Adjointe : à l'écran, et dite.
    fn echec_grave(&mut self, message: String) {
        journal::ecrire(&message);
        self.erreur = Some(message);
        self.dire(&format!("J'ai un problème avec {}. Le détail est à l'écran.", self.nommee().le));
    }

    /// Celle qui travaille derrière Eve, comme elle la nomme : l'Adjointe, ou Claude là où le dossier a sa consigne.
    fn nommee(&self) -> &'static Nommee {
        if self.propre { &CLAUDE } else { &ADJOINTE }
    }

    fn claude_fini(&mut self) {
        journal::ecrire("Claude Code s'est arrêté.");
        let en_plein_tour = self.tours > 0 && !self.prechauffage;
        self.cerveau = None;
        // Ses agents en arrière-plan se sont arrêtés avec lui.
        self.agents.clear();
        self.compte_rendu_attendu = None;
        self.init_ok = false;
        self.branchement = None;
        self.constructo_branche = false;
        self.rechauffer = false;
        self.rechauffages = 0;
        self.tours = 0;
        self.prechauffage = false;
        self.interrompu = false;
        self.chrono = None;
        self.attentes.clear();
        self.activite.clear();
        self.en_route.clear();
        self.en_cours.clear();
        self.flux.nouveau_tour();
        if self.quitter || self.erreur.is_some() {
            return;
        }
        let maintenant = Instant::now();
        self.relances.retain(|t| maintenant.duration_since(*t) < Duration::from_secs(120));
        if self.relances.len() >= 3 {
            self.echec_grave(format!("{} s'arrête sans cesse : voir le journal (eve.log).", self.nommee().le_maj));
            return;
        }
        // La première relance reprend la conversation : « et pour vendredi ? » garde son sens.
        self.reprendre = self.relances.is_empty() && self.session.is_some();
        self.relances.push(maintenant);
        let n = self.nommee();
        if en_plein_tour {
            self.dire(&format!(
                "{} s'est arrêté{} en pleine demande. Je {} relance ; redemande-moi dans un instant.",
                n.le_maj, n.e, n.la
            ));
        }
        self.systeme(&format!("{} s'est arrêté{} ; je {} relance.", n.le_maj, n.e, n.la));
        self.generation += 1;
        self.lancer_cerveau();
    }

    fn nouvelle_conversation(&mut self, titre: &str, oublier: bool) {
        self.couper();
        self.ecoute_suite = None;
        if let Some(c) = self.cerveau.take() {
            c.arreter();
        }
        self.agents.clear();
        self.compte_rendu_attendu = None;
        self.init_ok = false;
        self.branchement = None;
        self.constructo_branche = false;
        self.tours = 0;
        self.prechauffage = false;
        self.interrompu = false;
        self.chrono = None;
        self.attentes.clear();
        self.activite.clear();
        self.en_attente.clear();
        self.vus.clear();
        self.signatures.clear();
        self.en_route.clear();
        self.en_cours.clear();
        self.flux.nouveau_tour();
        self.session = None;
        self.seance_du = None;
        self.reprendre = false;
        self.rechauffer = false;
        self.rechauffages = 0;
        self.conversation = journal::horodatage();
        self.reprise = None;
        self.pieces_du_tour.clear();
        if oublier && self.reglages.memoire {
            memoire::oublier(&self.reglages.qui);
        }
        self.memoire = if self.reglages.memoire { memoire::charger(&self.reglages.qui) } else { Vec::new() };
        self.memoire_a_donner = !self.memoire.is_empty();
        self.ajouter("systeme", titre.to_string(), Vec::new());
        self.generation += 1;
        if self.dossier.is_some() && self.claude.is_some() {
            self.erreur = None;
            self.lancer_cerveau();
        }
    }

    // ---- Le veilleur -------------------------------------------------------------------------------------------

    fn nouvelle(&mut self, n: Nouvelle) {
        if n.nouvelle {
            let alertes = &self.reglages.alertes;
            let dire = match n.sorte {
                Sorte::Suspect => alertes.suspects,
                Sorte::Facture => alertes.factures,
                // Une numérisation de la caméra d'Eve elle-même : l'Adjointe la classe déjà, sans alerte de plus.
                Sorte::Scan if n.texte.contains("Numérisation Eve") => false,
                Sorte::Piece | Sorte::Scan => alertes.pieces,
                _ => false,
            };
            // Un courriel suspect s'affiche toujours, même sans alerte vocale.
            if dire || n.sorte == Sorte::Suspect {
                self.ajouter("alerte", n.texte.clone(), Vec::new());
            }
            if dire {
                let phrase = sans_point(&n.texte);
                self.alertes.push_back(match n.sorte {
                    Sorte::Suspect => {
                        format!("Attention : {phrase}. N'ouvre ni lien ni pièce avant de l'avoir vérifié.")
                    }
                    _ => format!("{phrase}."),
                });
            }
        }
        self.nouvelles.push(n);
        if self.nouvelles.len() > 300 {
            let trop = self.nouvelles.len() - 300;
            self.nouvelles.drain(..trop);
        }
    }

    // ---- Le battement --------------------------------------------------------------------------------------------

    fn tic(&mut self) {
        let maintenant = Instant::now();
        let jour = journal::date();
        if jour != self.jour {
            self.jour = jour;
            self.nouvelles.clear();
            self.dernier_point = None;
        }
        self.micro_et_oreille(maintenant);
        if self.ecoute && self.debut_ecoute.is_some_and(|d| maintenant.duration_since(d) > ECOUTE_MAX) {
            self.parler_fin();
        }
        if self.ecoute && self.parle && self.debut_ecoute.is_some_and(|d| maintenant.duration_since(d) > COUPE_APRES) {
            self.couper();
        }
        self.suivi_du_tour(maintenant);
        self.garder_au_chaud(maintenant);
        self.dire_les_alertes(maintenant);
        self.sonner_les_rappels(maintenant);
        self.surveiller_l_initialisation(maintenant);
        self.renouveler_la_conversation(maintenant);
        // L'émotion d'une réponse s'efface une minute et demie après, quand Eve ne parle plus.
        if !self.parle && self.emotion.is_some_and(|(_, depuis)| maintenant.duration_since(depuis) > EMOTION_DUREE) {
            self.emotion = None;
        }
        if self.attentes.front().is_some_and(|a| maintenant.duration_since(a.depuis) > ACCORD_DELAI) {
            self.systeme("Pas de réponse en trois minutes : l'écriture est refusée.");
            self.decider(false);
        }
        self.reessayer_le_micro(maintenant);
        self.surveiller_la_fenetre(maintenant);
        self.marque_de_sante(maintenant);
        self.mener_la_relance(maintenant);
        self.relire_la_precedente(maintenant);
        self.surveiller_la_connexion(maintenant);
        self.relire_les_reglages();
        self.suivre_claude();
    }

    /// L'écoute continue. Pendant qu'Eve parle, le micro est en sourdine — sauf, si l'interruption à la voix est
    /// permise, quand la phrase entendue ne peut valoir que pour un appel « Eve, … » : jamais pendant une demande
    /// d'accord ni après « Oui ? », où elle serait prise pour la réponse (la voix d'Eve ne doit jamais valoir
    /// « je confirme »).
    fn micro_et_oreille(&mut self, maintenant: Instant) {
        let echo = self.fin_parole.is_some_and(|f| maintenant.duration_since(f) < Duration::from_millis(600));
        let son = self.sourdine_jusqua.is_some_and(|f| maintenant < f);
        let appel_seul = self.reglages.interruption
            && self.reglages.eveil
            && self.attentes.is_empty()
            && self.ecoute_suite.is_none()
            && self.oui.is_none();
        let sourdine = son || ((self.parle || echo) && !appel_seul);
        let (phrases, en_parole) = {
            let Some(m) = &self.micro else { return };
            m.sourdine(sourdine);
            let mut v = Vec::new();
            while let Some(p) = m.prendre_phrase() {
                v.push(p);
            }
            (v, m.en_parole())
        };
        let prete = self.oreille_etat.0 == "prete";
        for p in phrases {
            if !prete {
                continue;
            }
            let suite = self.ecoute_suite.is_some_and(|f| maintenant < f);
            if suite || !self.attentes.is_empty() {
                self.ecoute_suite = None;
                if self.oreille.send(Demande::Commande { audio: p, continue_: true, depuis: maintenant }).is_ok() {
                    self.transcriptions += 1;
                } else {
                    self.oreille_perdue();
                }
            } else if self.oreille.send(Demande::Eveil { audio: p, depuis: maintenant }).is_err() {
                self.oreille_perdue();
            }
        }
        // Après « Oui ? » : une phrase commencée avant l'échéance compte encore.
        if self.ecoute_suite.is_some_and(|f| maintenant >= f) {
            self.ecoute_suite = if en_parole { Some(maintenant + Duration::from_millis(500)) } else { None };
        }
    }

    fn suivi_du_tour(&mut self, maintenant: Instant) {
        if self.tours == 0 || self.prechauffage || !self.attentes.is_empty() || self.parle {
            return;
        }
        let ecoule = self.debut_tour.map(|d| maintenant.duration_since(d)).unwrap_or_default();
        if !self.annonce && !self.dit_dans_le_tour && ecoule > ANNONCE_APRES {
            self.annonce = true;
            self.dire("Je m'en occupe.");
            return;
        }
        // Un travail qui s'allonge se dit : une fois à une minute, une fois à deux minutes et demie.
        let muet = maintenant.duration_since(self.dernier_signe);
        if ecoule.as_secs() >= 60 && self.rassure == 0 {
            self.rassure = 1;
            if muet > Duration::from_secs(45) && self.en_route.is_empty() {
                self.dire(&format!(
                    "Je n'ai pas de nouvelles {} depuis un moment ; je patiente. Dis « arrête » si tu veux.",
                    self.nommee().de
                ));
            } else {
                self.dire("C'est un peu long ; je suis toujours dessus.");
            }
        } else if ecoule.as_secs() >= 150 && self.rassure == 1 {
            self.rassure = 2;
            self.dire("Toujours en cours. Dis « arrête » si tu veux que j'arrête.");
        }
    }

    /// Un tour silencieux quand le contexte va refroidir : Eve est libre, l'Adjointe aussi, et c'est l'heure du bureau.
    fn garder_au_chaud(&mut self, maintenant: Instant) {
        let libre = self.init_ok && self.tours == 0 && self.en_attente.is_empty() && self.attentes.is_empty();
        let occupee = self.parle || self.ecoute || self.transcriptions > 0;
        if !self.reglages.au_chaud || !libre || occupee || !heures_de_bureau() {
            return;
        }
        if maintenant.duration_since(self.dernier_tour) < AU_CHAUD_APRES {
            return;
        }
        journal::ecrire("le contexte va refroidir : un tour silencieux le garde au chaud");
        let texte =
            "[Eve garde ton contexte au chaud — tour silencieux. Ne lance aucun outil ; réponds seulement « Prête. »]";
        self.en_attente.push_back(Message::silencieux(texte, true));
        self.suivant();
    }

    /// Les alertes : une cloche, puis la phrase, quand Eve n'est occupée ni à écouter, ni à parler, ni à attendre un
    /// accord — et seulement aux heures de bureau (la nuit, elles restent au fil).
    fn dire_les_alertes(&mut self, maintenant: Instant) {
        if let Some((quand, texte)) = self.alerte_a_dire.take() {
            if maintenant >= quand {
                // L'alerte est déjà au fil (`ajouter("alerte")`) : la voix seule, que la sourdine fait taire — sinon,
                // en sourdine, elle s'écrivait deux fois.
                self.dire_a_voix_haute(&texte);
            } else {
                self.alerte_a_dire = Some((quand, texte));
            }
            return;
        }
        if self.alertes.is_empty() {
            return;
        }
        if !heures_de_bureau() {
            self.alertes.clear();
            return;
        }
        // Ni pendant la fenêtre d'après « Eve » : la cloche mettrait le micro en sourdine, et la demande en cours y
        // perdrait son début.
        if self.parle
            || self.ecoute
            || self.transcriptions > 0
            || !self.attentes.is_empty()
            || self.ecoute_suite.is_some()
        {
            return;
        }
        if let Some(texte) = self.alertes.pop_front() {
            self.son(Son::Alerte);
            self.alerte_a_dire = Some((maintenant + Duration::from_millis(550), texte));
        }
    }

    fn reessayer_le_micro(&mut self, maintenant: Instant) {
        let sans_micro = self.micro.as_ref().is_none_or(Micro::en_panne);
        if !sans_micro || self.ecoute || maintenant.duration_since(self.micro_essai) < self.micro_delai {
            return;
        }
        self.micro_essai = maintenant;
        match Micro::ouvrir() {
            Ok(m) => {
                journal::ecrire(&format!("micro : {}", m.nom));
                m.ecoute_continue(self.reglages.eveil);
                // Un micro qui clignote ne noie pas le fil : on ne l'annonce que s'il change.
                if m.nom != self.micro_nom {
                    self.systeme(&format!("Micro branché : {}", m.nom));
                }
                self.micro_nom = m.nom.clone();
                self.micro = Some(m);
                self.micro_signale = false;
                self.micro_delai = Duration::from_secs(5);
            }
            Err(_) => {
                if self.micro.take().is_some() {
                    self.systeme("Le micro s'est débranché.");
                }
                self.micro_delai = (self.micro_delai * 2).min(Duration::from_secs(60));
            }
        }
    }

    /// La fenêtre fermée (la page le dit en partant), Eve se ferme ; un flux coupé sans un mot (Edge planté) attend un
    /// peu plus — et beaucoup plus pendant un travail ou une demande d'accord.
    fn surveiller_la_fenetre(&mut self, maintenant: Instant) {
        if !self.avec_fenetre {
            return;
        }
        if self.ecran.clients() > 0 {
            if !self.a_eu_client {
                self.a_eu_client = true;
                if self.reglages.devant {
                    fenetre::premier_plan(true);
                }
            }
            self.sans_client = None;
            self.fenetre_fermee = false;
        } else if self.a_eu_client {
            let depuis = *self.sans_client.get_or_insert(maintenant);
            let delai = if self.fenetre_fermee {
                5
            } else if self.tours > 0 || !self.attentes.is_empty() {
                600
            } else {
                90
            };
            if maintenant.duration_since(depuis) > Duration::from_secs(delai) {
                journal::ecrire("La fenêtre est fermée : Eve se ferme.");
                self.quitter = true;
            }
        }
    }

    /// L'espace d'actions d'abord (un outil hors des privilèges ne part jamais), puis la garde (les jetons et les clés,
    /// jamais), puis les accords d'Eve, que la personne garde ou retire (le privilège `accords`) : [`garde::decider`].
    fn decision(&self, nom: &str, entree: &Value) -> Decision {
        garde::decider(&self.espace, nom, entree, self.tour_a_la_voix)
    }

    /// Avant qu'un fichier change : sa copie, pour « annule la dernière modification ».
    fn garder_une_copie(&mut self, nom: &str, entree: &Value) {
        if let Some(chemin) = annuler::fichier_ecrit(nom, entree)
            && let Err(e) = self.annulations.garder(self.tour_numero, &chemin)
        {
            journal::ecrire(&format!("copie d'avant impossible ({}) : {e}", chemin.display()));
        }
    }

    /// « Eve, annule la dernière modification » : les fichiers du dernier tour qui en a changé, remis comme avant.
    fn annuler_la_derniere_modification(&mut self) {
        if self.annulations.vide() {
            self.dire("Je n'ai aucune modification de fichier à annuler depuis mon démarrage.");
            return;
        }
        let (remis, erreurs) = self.annulations.annuler_le_dernier_tour();
        for e in &erreurs {
            journal::ecrire(&format!("annulation : {e}"));
        }
        // Ce qui n'a pas pu être remis reste à annuler (`Annulations::annuler_le_dernier_tour`) : la personne le sait.
        let pas_remis = || {
            format!(
                "Pas remis :\n{}\nFerme le fichier s'il est ouvert, ou rebranche son lecteur, puis redis « annule la \
                 dernière modification » : je reprendrai ces fichiers-là.",
                erreurs.join("\n")
            )
        };
        if remis.is_empty() {
            self.systeme(&pas_remis());
            self.dire("Je n'ai rien pu remettre comme avant. Le détail est à l'écran.");
            return;
        }
        let liste: Vec<String> = remis
            .iter()
            .map(|(c, comme_avant, gardee)| {
                let comment = if *comme_avant { "remis comme avant" } else { "retiré : il venait d'être créé" };
                let garde = gardee
                    .as_ref()
                    .and_then(|g| g.file_name())
                    .map(|n| format!(" ; gardé sous « {} »", n.to_string_lossy()))
                    .unwrap_or_default();
                format!("{} ({comment}{garde})", c.display())
            })
            .collect();
        journal::ecrire(&format!("annulé : {}", liste.join(" ; ")));
        self.systeme(&format!(
            "Annulé :\n{}\nLa version remplacée de chaque fichier, et ce qui a été retiré, sont gardés dans {}.",
            liste.join("\n"),
            self.annulations.dossier().display()
        ));
        self.note_pour_l_adjointe = Some(format!(
            "[Eve a annulé, à la demande de la personne, tes dernières modifications de fichiers : {}. Ces fichiers \
             sont revenus à leur état d'avant ; n'y reviens pas sans qu'on te le demande.]",
            liste.join(" ; ")
        ));
        let n = remis.len();
        let s = if n > 1 { "s" } else { "" };
        if erreurs.is_empty() {
            self.dire(&format!("C'est annulé : {n} fichier{s} remis comme avant."));
        } else {
            self.systeme(&pas_remis());
            self.dire(&format!("J'ai remis {n} fichier{s} comme avant, mais pas tout. Le détail est à l'écran."));
        }
    }

    /// Une écriture qui a tourné (ce que l'ancienne garde aurait fait confirmer) : notée pour le résumé de la journée.
    fn noter_action(&self, nom: &str, entree: &Value) {
        let Decision::Confirmer { quoi, detail } = garde::examiner(nom, entree) else { return };
        let detail = garde::ecourter(&detail.split_whitespace().collect::<Vec<_>>().join(" "), 300);
        let ligne = format!("{}  {quoi} — {detail}\n", journal::heure());
        let dossier = dossier_local().join("actions");
        let ecrit = std::fs::create_dir_all(&dossier).and_then(|_| {
            use std::io::Write;
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(dossier.join(format!("{}.txt", journal::date())))?
                .write_all(ligne.as_bytes())
        });
        if let Err(e) = ecrit {
            journal::ecrire(&format!("action non notée : {e}"));
        }
    }

    /// La carte aperçu, « Ouvrir en grand » : dans le programme du poste. Seulement ce qui s'affiche en aperçu.
    fn ouvrir_fichier(&mut self, chemin: &str) {
        let c = PathBuf::from(chemin);
        if ecran::sorte_d_apercu(&c).is_none() || !c.is_file() {
            self.systeme("Ce fichier ne s'ouvre pas d'ici.");
            return;
        }
        if let Err(e) = Command::new("explorer.exe").arg(&c).spawn() {
            self.systeme(&format!("Le fichier ne s'ouvre pas : {e}"));
        }
    }

    // ---- L'écran et la voix ----------------------------------------------------------------------------------------

    /// Une phrase qu'Eve ne fait que dire (« Je n'ai rien entendu. », une alerte) : en sourdine, elle s'écrit à l'écran.
    fn dire(&mut self, texte: &str) {
        if self.reglages.sourdine {
            self.systeme(texte);
        } else {
            self.dire_a_voix_haute(texte);
        }
    }

    /// Une phrase déjà écrite à l'écran (la réponse) : en sourdine, rien de plus ; le micro n'a pas à se taire. Le
    /// numéro de la parole, si elle se dit.
    fn dire_a_voix_haute(&mut self, texte: &str) -> Option<u64> {
        if self.reglages.sourdine {
            return None;
        }
        self.parole_id += 1;
        self.parle = true;
        self.voix.dire(self.parole_id, texte);
        let maintenant = Instant::now();
        self.dit_recemment.retain(|(quand, _)| maintenant.duration_since(*quand) < DIT_RECEMMENT);
        self.dit_recemment.push_back((maintenant, texte.to_string()));
        Some(self.parole_id)
    }

    /// Une phrase de la réponse (ou un commentaire du travail) : elle prend la file de la voix, sans rien couper.
    fn dire_phrase(&mut self, texte: &str) -> Option<u64> {
        self.dit_dans_le_tour = true;
        self.dernier_dit = Some(Instant::now());
        self.dire_a_voix_haute(texte)
    }

    /// Eve se tait tout de suite (ce qui attendait d'être dit est oublié, et l'émotion de ses passages).
    fn couper(&mut self) {
        self.voix.taire();
        self.parle = false;
        self.oui = None;
        self.fin_parole = Some(Instant::now());
        self.passages.couper();
    }

    /// Un petit son ; le micro en sourdine pendant ce temps, et un peu après (l'écho de la pièce) : Eve ne s'entend pas
    /// elle-même.
    fn son(&mut self, s: Son) {
        sons::jouer(s);
        self.sourdine_jusqua = Some(Instant::now() + sons::duree(s) + Duration::from_millis(150));
    }

    fn oreille_perdue(&mut self) {
        self.oreille_etat = ("erreur", "l'oreille s'est arrêtée".into());
    }

    fn ajouter(&mut self, de: &'static str, texte: String, pieces: Vec<String>) {
        self.fil.push(Ligne { de, texte, heure: journal::heure(), pieces, ancien: false, tour: String::new() });
        self.fil_rev += 1;
        if self.fil.len() > FIL_MAX {
            let trop = self.fil.len() - FIL_MAX;
            self.fil.drain(..trop);
        }
    }

    fn systeme(&mut self, texte: &str) {
        self.ajouter("systeme", texte.to_string(), Vec::new());
    }

    /// Le garde-fou de l'oreille ([`parole::juger_la_transcription`]) : une phrase douteuse ne part pas à Claude — Eve
    /// dit qu'elle a mal entendu et la met dans la saisie, à corriger ; sa propre voix revenue par le micro s'ignore ;
    /// une boucle se replie. Rend ce qui part. Une réponse à une demande d'accord a ses propres règles : elle passe.
    /// `depuis` : la fin de la parole entendue ; sa voix ne se cherche que si Eve parlait pendant que le son se prenait
    /// (on lui répond souvent avec ses mots à elle).
    fn passer_l_oreille(&mut self, t: String, parole_s: f32, calcul_s: f32, depuis: Instant) -> Option<String> {
        if t.is_empty() || !self.attentes.is_empty() {
            return Some(t);
        }
        let maintenant = Instant::now();
        // Le début du son (une seconde de marge ; la demande d'après « Eve » n'a pas de durée : quatre secondes).
        let duree = if parole_s > 0.0 { parole_s + 1.0 } else { 4.0 };
        let debut = depuis.checked_sub(Duration::from_secs_f32(duree)).unwrap_or(depuis);
        let parlait = self.parle || self.fin_parole.is_some_and(|f| f >= debut);
        let dites: Vec<String> = if parlait {
            self.dit_recemment
                .iter()
                .filter(|(quand, _)| maintenant.duration_since(*quand) < DIT_RECEMMENT)
                .map(|(_, p)| p.clone())
                .collect()
        } else {
            Vec::new()
        };
        // La carte graphique : une phrase y prend moins de deux secondes, une boucle bien plus (sur le processeur, une
        // phrase courte en prend déjà sept).
        let sur_la_carte = self.oreille_etat.1.contains("sur la carte graphique");
        match parole::juger_la_transcription(&t, parole_s, calcul_s, sur_la_carte, &dites) {
            Transcription::Juste(j) => {
                if j != t {
                    journal::ecrire(&format!("repliée (une boucle de l'oreille) : {j}"));
                }
                Some(j)
            }
            Transcription::Douteuse { texte, raison } => {
                journal::ecrire(&format!("douteuse ({raison}) : rien n'est parti à Claude"));
                self.systeme(&format!("Mal entendu ({raison}) : rien n'est parti. La phrase est dans la saisie."));
                let numero = self.a_corriger.as_ref().map_or(1, |(n, _)| n + 1);
                self.a_corriger = Some((numero, texte));
                self.dire("Je n'ai pas bien entendu. Redis-le, ou corrige-le à l'écran.");
                None
            }
            Transcription::MaVoix => {
                journal::ecrire("ma propre voix, revenue par le micro : ignorée");
                None
            }
        }
    }

    /// La phrase de la panne vue dans ce tour (`parole::phrase_de_panne`), avec l'heure où la limite se rouvre ; la
    /// panne s'oublie (le tour suivant a les siennes).
    fn phrase_de_la_panne(&mut self) -> Option<(String, String)> {
        let panne = std::mem::take(&mut self.panne);
        self.reessai_dit = false;
        let reprise = self.limite.and_then(rappels::heure_locale).map(|a| rappels::heure_dite(&a, &journal::date()));
        parole::phrase_de_panne(&panne, reprise.as_deref(), self.reglages.modele.contains("opus"))
    }

    /// La limite d'utilisation du compte, que Claude Code dit à chaque appel : atteinte, Eve le dit une fois (et
    /// jusqu'à quand) ; presque atteinte, une ligne au fil, une fois ; ouverte, rien.
    fn suivre_la_limite(&mut self, etat: &str, reprise: Option<i64>, sorte: &str, part: Option<f64>) {
        if etat == "allowed" {
            self.limite = None;
            return;
        }
        let quand = reprise.and_then(rappels::heure_locale).map(|a| rappels::heure_dite(&a, &journal::date()));
        let pourcent =
            part.map(|p| if p <= 1.0 { p * 100.0 } else { p }).map(|p| format!(" ({p:.0} %)")).unwrap_or_default();
        journal::ecrire(&format!(
            "limite de Claude : {etat}, {sorte}{pourcent}{}",
            quand.as_ref().map(|q| format!(", se rouvre {q}")).unwrap_or_default()
        ));
        // Atteinte : la fin du tour le dira, avec l'heure où elle se rouvre.
        if etat == "rejected" {
            self.limite = reprise;
            self.panne = "rate_limit".into();
            return;
        }
        let cle = (etat.to_string(), reprise);
        if self.limite_dite.as_ref() != Some(&cle) {
            self.limite_dite = Some(cle);
            let quand = quand.map(|q| format!(" ; elle se rouvre {q}")).unwrap_or_default();
            self.systeme(&format!("Ma limite d'utilisation de Claude est presque atteinte{pourcent}{quand}."));
        }
    }

    /// La séance de Claude au fichier `seance.json` du dossier local : son jour, son profil, son numéro, et l'instant de
    /// son dernier tour. Un redémarrage dans l'heure la reprend ([`seance_a_reprendre`]).
    fn noter_la_seance(&self) {
        let Some(session) = &self.session else { return };
        let seance = json!({
            "jour": self.seance_du.clone().unwrap_or_else(journal::date),
            "profil": self.profil.as_ref().map(|p| p.nom.as_str()).unwrap_or(""),
            "session": session,
            "vu": journal::date_et_heure(),
        });
        let chemin = dossier_local().join("seance.json");
        if let Err(e) = crate::reglages::ecrire_d_un_geste(&chemin, seance.to_string().as_bytes()) {
            journal::ecrire(&format!("seance.json ne s'écrit pas : {e}"));
        }
    }

    /// Une conversation par jour : la séance d'un autre jour, passé [`NOUVELLE_JOURNEE_A`] h, au premier moment libre
    /// après [`CALME_AVANT_LA_NOUVELLE`] de calme, laisse la place à une neuve — les tours ne s'alourdissent plus de
    /// tout ce qui s'est dit les jours d'avant (la mémoire courte fait le pont). Les conversations laissées par les
    /// autres profils s'oublient aussi.
    fn renouveler_la_conversation(&mut self, maintenant: Instant) {
        let aujourdhui = journal::date();
        let Some(du) = &self.seance_du else { return };
        let heure: u32 = journal::heure().get(..2).and_then(|h| h.parse().ok()).unwrap_or(0);
        let libre = self.init_ok
            && self.tours == 0
            && self.en_attente.is_empty()
            && self.attentes.is_empty()
            && self.agents.is_empty()
            && self.compte_rendu_attendu.is_none()
            && !self.parle
            && !self.ecoute;
        if *du == aujourdhui
            || heure < NOUVELLE_JOURNEE_A
            || !libre
            || maintenant.duration_since(self.dernier_tour) < CALME_AVANT_LA_NOUVELLE
        {
            return;
        }
        journal::ecrire(&format!("nouvelle journée : la conversation du {du} laisse la place à une neuve"));
        self.laisses.oublier();
        self.nouvelle_conversation("— Nouvelle journée : une conversation neuve —", false);
    }

    /// Claude Code qui ne répond pas à `initialize` en [`INIT_DELAI`] : arrêté et relancé, comme s'il s'était arrêté
    /// (trois fois en deux minutes, Eve le dit et cesse).
    fn surveiller_l_initialisation(&mut self, maintenant: Instant) {
        if self.init_ok || !self.init_attendue.is_some_and(|t| maintenant.duration_since(t) > INIT_DELAI) {
            return;
        }
        self.init_attendue = None;
        journal::ecrire("Claude Code ne répond pas à l'initialisation en deux minutes : je le relance");
        if let Some(c) = self.cerveau.take() {
            c.tuer();
        }
        self.claude_fini();
    }

    /// Le point du jour ([`crate::centre::Centre::point_du_jour`]) : le tableau de bord lu d'un coup — les fichiers
    /// d'`activite\`, la comptabilité nommée, les rappels.
    fn point_du_jour(&self) -> Vec<String> {
        let dossier = self.reglages.dossier_adjointe();
        let centre =
            crate::centre::Centre::lu_maintenant(dossier.as_deref(), &dossier_local(), &self.reglages.comptabilite);
        centre.point_du_jour(&journal::date(), &journal::heure())
    }

    /// Un rappel ou une minuterie dits (la voie rapide) : posés dans rappels.json, puis l'heure retenue, au fil et à
    /// voix haute.
    fn poser_un_rappel_dit(&mut self, demande: &str, r: parole::RappelDit) {
        let maintenant = journal::date_et_heure();
        let Some(a) = rappels::heure_du_rappel(r.quand, &maintenant) else {
            let non = "Je n'arrive pas à placer cette heure : redis-la autrement.";
            self.repondre_sans_claude(demande, non, non);
            return;
        };
        let texte = match (r.minuterie, r.quand) {
            (true, parole::Quand::Dans(s)) => {
                let duree = rappels::duree_en_mots(s);
                if r.texte.is_empty() {
                    format!("La minuterie de {duree} est finie.")
                } else {
                    format!("{} : la minuterie de {duree} est finie.", majuscule(&r.texte))
                }
            }
            _ if r.texte.is_empty() => "c'est l'heure.".to_string(),
            _ => r.texte.clone(),
        };
        let quand = rappels::heure_dite(&a, &journal::date());
        let quand = if quand.starts_with(|c: char| c.is_ascii_digit()) { format!("à {quand}") } else { quand };
        match self.rappels.poser(a, texte.clone(), r.minuterie) {
            Ok(_) if r.minuterie => {
                let ecrit = format!("Minuterie lancée : elle sonne **{quand}**.");
                self.repondre_sans_claude(demande, &ecrit, &format!("C'est parti. Elle sonne {quand}."));
            }
            Ok(_) => {
                let ecrit = format!("Rappel noté, **{quand}** : {texte}");
                self.repondre_sans_claude(demande, &ecrit, &format!("C'est noté. Je te le rappelle {quand}."));
            }
            Err(e) => {
                journal::ecrire(&format!("rappel non posé : {e}"));
                let non = "Je n'arrive pas à noter ce rappel. Le détail est au journal.";
                self.repondre_sans_claude(demande, non, non);
            }
        }
    }

    /// Une carte « rappel » de Claude (`{"type":"rappel","a":"AAAA-MM-JJ HH:MM","texte":"…"}`) : posée dans
    /// rappels.json. Sa réponse a déjà dit l'heure ; une carte fausse va au journal.
    fn poser_un_rappel_de_claude(&mut self, json: &str) {
        let Ok(v) = serde_json::from_str::<Value>(json).or_else(|_| json5::from_str::<Value>(json)) else { return };
        if v["type"] != "rappel" {
            return;
        }
        let texte = v["texte"].as_str().unwrap_or("").trim();
        let texte = if texte.is_empty() { "c'est l'heure." } else { texte };
        match v["a"].as_str().and_then(rappels::heure_ecrite) {
            Some(a) => match self.rappels.poser(a.clone(), texte.to_string(), false) {
                Ok(_) => journal::ecrire(&format!("rappel posé par Claude, {a} : {texte}")),
                Err(e) => journal::ecrire(&format!("rappel de Claude non posé : {e}")),
            },
            None => journal::ecrire(&format!("une carte « rappel » sans heure lisible : {json}")),
        }
    }

    /// Les rappels dus, une fois par seconde : la cloche, le rappel au fil (en alerte) et à voix haute — même la nuit ;
    /// en sourdine, la cloche et le fil. Un rappel manqué (Eve fermée à son heure) dit l'heure qu'il avait.
    fn sonner_les_rappels(&mut self, maintenant: Instant) {
        if self.rappels_vus.is_some_and(|t| maintenant.duration_since(t) < Duration::from_secs(1)) {
            return;
        }
        self.rappels_vus = Some(maintenant);
        self.rappels.relire();
        let heure = journal::date_et_heure();
        for r in self.rappels.sonner(&heure) {
            let mut phrase = r.phrase();
            if rappels::secondes_entre(&r.a, &heure) > 120 {
                let prevu = rappels::heure_dite(&r.a, &journal::date());
                let prevu = if prevu.starts_with(|c: char| c.is_ascii_digit()) { format!("à {prevu}") } else { prevu };
                phrase = format!("{phrase} (prévu {prevu}, quand j'étais fermée)");
            }
            journal::ecrire(&format!("rappel sonné : {phrase}"));
            self.son(Son::Alerte);
            self.ajouter("alerte", phrase.clone(), Vec::new());
            self.dire_a_voix_haute(&phrase);
        }
    }

    /// « La suite » : la page suivante de la dernière réponse, dite ; si d'autres suivent, Eve le dit. En sourdine, ou
    /// sans rien de plus à lire, elle le dit aussi.
    fn lire_la_suite(&mut self) {
        if self.reglages.sourdine {
            self.systeme("Je suis en sourdine : dis « parle » pour m'entendre.");
            return;
        }
        let Some(page) = self.pages.get(self.page).cloned() else {
            self.dire(if self.pages.is_empty() { "Je n'ai rien à lire." } else { "C'est tout." });
            return;
        };
        journal::ecrire(&format!("la suite, sans Claude : page {} sur {}", self.page + 1, self.pages.len()));
        self.page += 1;
        self.couper();
        self.dire_a_voix_haute(&page);
        if self.page < self.pages.len() {
            self.dire_a_voix_haute("Dis « la suite » pour continuer.");
        }
    }

    /// « Répète » : la dernière page dite de la dernière réponse, de nouveau.
    fn repeter(&mut self) {
        if self.reglages.sourdine {
            self.systeme("Je suis en sourdine : dis « parle » pour m'entendre.");
            return;
        }
        let Some(page) = self.page.checked_sub(1).and_then(|k| self.pages.get(k)).cloned() else {
            self.dire("Je n'ai rien à répéter.");
            return;
        };
        journal::ecrire("répète, sans Claude");
        self.couper();
        self.dire_a_voix_haute(&page);
    }

    /// Une réponse d'Eve elle-même à `demande`, sans Claude (la voie rapide) : `ecrit` au fil et à l'historique, `dit` à
    /// voix haute (en sourdine, écrite seulement).
    fn repondre_sans_claude(&mut self, demande: &str, ecrit: &str, dit: &str) {
        journal::ecrire(&format!("réponse d'Eve, sans Claude : {}", garde::ecourter(ecrit, 400)));
        self.historiser(demande, ecrit, "");
        self.ajouter("eve", ecrit.to_string(), Vec::new());
        self.dire_a_voix_haute(dit);
    }

    /// L'état à l'écran : son nom (la classe de l'orbe), son libellé, et l'aide (les touches entre crochets).
    fn etat(&self) -> (&'static str, String, String) {
        let touche = self.reglages.touche().1;
        if let Some(e) = &self.erreur {
            return ("erreur", "Un problème".into(), e.clone());
        }
        if self.ecoute {
            return ("ecoute", "J'écoute…".into(), "Relâche la touche quand tu as fini".into());
        }
        if self.transcriptions > 0 {
            return ("transcription", "Je transcris…".into(), String::new());
        }
        if !self.attentes.is_empty() {
            return (
                "confirmation",
                "J'attends ton accord".into(),
                "Dis « je confirme » ou « annule » — ou clique".into(),
            );
        }
        // Après « Eve » seul, la phrase suivante est la demande : l'écran le dit (en sourdine, rien d'autre ne le dit).
        if self.ecoute_suite.is_some_and(|f| Instant::now() < f) {
            return ("ecoute", "Oui ?".into(), "Dis ta demande".into());
        }
        if self.parle {
            let aide = if self.reglages.interruption && self.reglages.eveil {
                format!("Dis « Eve, stop » — ou maintiens [{touche}] — pour me couper")
            } else {
                format!("Maintiens [{touche}] pour me couper la parole")
            };
            return ("parole", "Je réponds".into(), aide);
        }
        if self.tours > 0 && (!self.prechauffage || !self.en_attente.is_empty()) {
            // Le temps écoulé, pour qu'on voie que le travail avance.
            let s = if self.prechauffage { 0 } else { self.debut_tour.map(|d| d.elapsed().as_secs()).unwrap_or(0) };
            let libelle = match s {
                0..=2 => "Je m'en occupe…".to_string(),
                3..=59 => format!("Je m'en occupe… {s} s"),
                _ => format!("Je m'en occupe… {} min {:02}", s / 60, s % 60),
            };
            return ("reflexion", libelle, "Dis « arrête » — ou clique Arrêter — pour interrompre".into());
        }
        if (self.prechauffage && !self.au_chaud) || !self.init_ok {
            return ("demarrage", "Je me prépare…".into(), "Tu peux déjà écrire ta demande".into());
        }
        if self.oreille_etat.0 == "chargement" {
            return ("demarrage", "Presque prête…".into(), "L'oreille se charge ; tu peux déjà écrire".into());
        }
        // La sourdine se voit : sans elle, une Eve muette semble en panne (une sourdine mise le soir, oubliée le matin).
        if self.reglages.sourdine {
            return (
                "pret",
                "Prête · en sourdine".into(),
                "Je réponds par écrit — dis « parle » pour m'entendre".into(),
            );
        }
        if self.reglages.eveil && self.micro.is_some() {
            return ("pret", "Prête".into(), format!("Dis « Eve… » — ou maintiens [{touche}] pour parler"));
        }
        ("pret", "Prête".into(), format!("Maintiens [{touche}] — ou le bouton, ou [Espace] — pour parler"))
    }

    /// Le portrait publié à la fenêtre ; au plus toutes les 80 ms (le battement publie ce qui a attendu).
    fn publier(&mut self, force: bool) {
        if !force && self.publie_a.elapsed() < PUBLICATION_MIN {
            return;
        }
        self.publie_a = Instant::now();
        let (etat, libelle, aide) = self.etat();
        let niveau =
            if self.ecoute { self.micro.as_ref().map(|m| (m.niveau() * 6.0).min(1.0)).unwrap_or(0.0) } else { 0.0 };
        let a = &self.reglages.alertes;
        // La version en place ; le bouton des réglages qui revient à la précédente, grisé quand une relance attend.
        let version = json!({
            "texte": mise_a_jour::version(),
            "empreinte": mise_a_jour::empreinte(),
            "precedente": self.precedente,
            "relance": self.relance.is_some() || self.relance_dite.is_some(),
        });
        let s = json!({
            "etat": etat,
            "libelle": libelle,
            "aide": aide,
            "niveau": niveau,
            // L'émotion de la réponse, que montre le visage d'Eve (son portrait).
            "emotion": self.emotion.map(|(e, _)| e.nom()),
            "qui": self.reglages.qui,
            "theme": self.reglages.theme,
            // La comptabilité que lit la fenêtre (l'encaisse, les échéances fiscales) : son dossier, ou rien.
            "comptabilite": self.reglages.comptabilite,
            "nom": crate::reglages::nom_affiche(),
            // Le profil où travaille Claude Code (celui d'office : le nom d'Eve), et tous, pour le menu de la barre.
            "profil": self.nom_du_profil(),
            "profils": self.profils_publies(),
            "propre": self.propre,
            "entreprise": self.reglages.nom_entreprise(),
            "base": self.base.0,
            "base_detail": self.base.1,
            "micro": self.micro.as_ref().map(|m| m.nom.clone()),
            "oreille": self.oreille_etat.0,
            "oreille_detail": self.oreille_etat.1,
            "accord": self.attentes.front().map(|x| json!({
                "numero": x.numero, "quoi": majuscule(&x.quoi), "detail": x.detail, "restants": self.attentes.len() - 1 })),
            "activite": self.activite,
            "outils": self.outils,
            "pieces": self.pieces.iter().map(|p| json!({ "id": p.id, "nom": p.nom, "taille": p.taille })).collect::<Vec<_>>(),
            "a_corriger": self.a_corriger.as_ref().map(|(numero, texte)| json!({ "numero": numero, "texte": texte })),
            "fil_rev": self.fil_rev,
            "fil": self.fil.iter().map(|l| json!({
                "de": l.de, "texte": l.texte, "heure": l.heure, "pieces": l.pieces, "ancien": l.ancien, "tour": l.tour }))
                .collect::<Vec<_>>(),
            "en_cours": if self.en_cours.trim().is_empty() || self.prechauffage { Value::Null } else { json!({ "texte": self.en_cours }) },
            "voix": { "liste": self.voix_liste, "choisie": self.voix_choisie },
            "reglages": {
                "eveil": self.reglages.eveil,
                "interruption": self.reglages.interruption,
                "sourdine": self.reglages.sourdine,
                "alertes": { "suspects": a.suspects, "factures": a.factures, "pieces": a.pieces, "rendez_vous": a.rendez_vous },
                "devant": self.reglages.devant,
                "demarrage": self.demarrage_windows,
                "historique": self.reglages.historique,
                "raccourci": "Ctrl+Alt+E",
                // Le cerveau : la rubrique « Cerveau » des réglages coche ceux qu'Eve offre (reglages::MODELES, EFFORTS).
                "modele": self.reglages.modele,
                "effort": self.reglages.effort,
            },
            "eveil": self.reglages.eveil,
            "touche": self.reglages.touche().1,
            "camera": self.camera,
            "camera_ordre": { "n": self.camera_ordre.0, "geste": self.camera_ordre.1 },
            // Constructo AI : le bouton des réglages (« Se connecter à Constructo AI », le compte, « Se déconnecter »).
            "constructo": self.etat_constructo(),
            "version": version,
        })
        .to_string();
        if s != self.dernier_publie {
            self.ecran.publier(&s);
            self.dernier_publie = s;
        }
    }
}

/// Le bloc des quinze jours du calendrier, joint à une question de calendrier (et à l'essai `--essai-claude`).
pub fn bloc_calendrier(lu: &str, calendrier: &[Jour]) -> String {
    let (Some(premier), Some(dernier)) = (calendrier.first(), calendrier.last()) else { return String::new() };
    let mut s = format!(
        "\n\n[Calendrier en ligne, relu par Eve à {lu} — du {} au {} ; un jour sans ligne n'a rien au calendrier :",
        premier.nom, dernier.nom
    );
    for j in calendrier.iter().filter(|j| !j.evenements.is_empty()) {
        let liste: Vec<String> = j
            .evenements
            .iter()
            .map(|e| {
                let mut x = String::new();
                if !e.heure.is_empty() {
                    x.push_str(&e.heure);
                    x.push_str(" · ");
                }
                x.push_str(&e.titre);
                if !e.detail.is_empty() {
                    x.push_str(" — ");
                    x.push_str(&garde::ecourter(&e.detail, 110));
                }
                if !e.sorte.is_empty() {
                    x.push_str(&format!(" ({})", e.sorte));
                }
                x
            })
            .collect();
        s.push_str(&format!("\n- {} : {}", j.nom, liste.join(" ; ")));
    }
    s.push(']');
    s
}

/// Les mesures d'une demande, au journal (« mesures du tour »).
fn noter_les_mesures(c: &Chrono) {
    let t = c.temps();
    if !t.vide() {
        journal::ecrire(&t.ligne());
    }
}

fn corps_de_decision(a: &Attente, oui: bool) -> Value {
    match (a.sorte, oui) {
        (Sorte2::Crochet, true) => protocole::crochet_accord("Accordé par la personne, dans Eve."),
        (Sorte2::Crochet, false) => protocole::crochet_refus("La personne a refusé dans Eve : rien n'a été écrit."),
        (Sorte2::Permission, true) => protocole::permission_accord(&a.entree),
        (Sorte2::Permission, false) => {
            protocole::permission_refus("La personne a refusé dans Eve : rien n'a été écrit.")
        }
    }
}

/// Ce que fait l'Adjointe, en quelques mots, d'après l'outil qu'elle prend (et une sorte, pour ne pas le redire).
fn commentaire(nom: &str, entree: &Value) -> Option<(&'static str, &'static str)> {
    if nom.starts_with("mcp__entrepreneur-general__") {
        return Some(("base", "Je consulte la base."));
    }
    // Constructo AI, par la connexion d'Eve ou par le connecteur de claude.ai : ce que touche l'outil, d'après son nom.
    if let Some(n) = garde::outil_erp(nom) {
        let dit = |m: &str| n.contains(m);
        return Some(if dit("email") {
            if dit("brouillon") {
                ("brouillon", "Je prépare le brouillon.")
            } else {
                ("courriels", "Je regarde les courriels.")
            }
        } else if dit("facture") || dit("paiement") || dit("comptab") || dit("banque") || dit("ecriture") {
            ("compta", "Je regarde la comptabilité.")
        } else if dit("devis") || dit("soumission") {
            ("soumissions", "Je regarde les soumissions.")
        } else if dit("projet") || dit("suivi") || dit("bon_travail") || n.starts_with("bt_") {
            ("projets", "Je regarde les projets.")
        } else if dit("employe") || dit("pointage") {
            ("employes", "Je regarde les employés.")
        } else if n.starts_with("calculer") {
            ("calcul", "Je fais le calcul.")
        } else {
            ("constructo", "Je consulte Constructo AI.")
        });
    }
    match nom {
        "Bash" | "PowerShell" => {
            let c = entree.get("command").and_then(Value::as_str).unwrap_or("").to_lowercase();
            let dit = |m: &str| c.contains(m);
            if dit("outlook_mail.py") {
                if dit(" draft") || dit(" reply") {
                    Some(("brouillon", "Je prépare le brouillon."))
                } else {
                    Some(("courriels", "Je regarde les courriels."))
                }
            } else if dit("calendrier") {
                Some(("calendrier", "Je regarde le calendrier."))
            } else if dit("compta_base.py") || dit("revenus_base.py") {
                Some(("compta", "Je cherche dans la comptabilité."))
            } else if dit("demandes_base.py") || dit("fournisseurs_base.py") {
                Some(("demandes", "Je regarde les demandes de prix."))
            } else if dit("soumission_base.py") {
                Some(("soumissions", "Je regarde les soumissions."))
            } else if dit("classer_courriels.py") {
                Some(("classer", "Je range les courriels."))
            } else if dit("matin.py") {
                Some(("matin", "Je fais le tour du poste."))
            } else {
                None
            }
        }
        "Read" | "Glob" | "Grep" => Some(("dossiers", "Je cherche dans les dossiers.")),
        "WebSearch" | "WebFetch" => Some(("web", "Je cherche en ligne.")),
        _ => None,
    }
}

/// Un titre du calendrier dit à voix haute : ses tirets deviennent des virgules.
fn oral(t: &str) -> String {
    t.replace(" — ", ", ").replace(" – ", ", ").replace(" - ", ", ").trim().to_string()
}

fn sans_point(t: &str) -> &str {
    t.trim().trim_end_matches(['.', ' '])
}

/// Le texte d'une réponse sans ses cartes (leur JSON n'a rien à faire dans la mémoire).
fn sans_cartes(t: &str) -> String {
    let mut sortie = String::new();
    let mut dans = false;
    for l in t.lines() {
        if l.trim_start().starts_with("```") {
            dans = !dans;
            continue;
        }
        if !dans {
            sortie.push_str(l);
            sortie.push('\n');
        }
    }
    sortie
}

/// La séance de Claude à reprendre au démarrage (`seance.json` du dossier local) : celle de ce jour, du même profil
/// (`profil` : son nom ; `None` : celui d'office), dont le dernier tour date de moins de [`REPRISE_MAX`] secondes.
fn seance_a_reprendre(profil: Option<&str>) -> Option<String> {
    let texte = std::fs::read_to_string(dossier_local().join("seance.json")).ok()?;
    seance_valable(&serde_json::from_str(&texte).ok()?, profil, &journal::date_et_heure())
}

/// [`seance_a_reprendre`], à l'instant `maintenant` (« AAAA-MM-JJ HH:MM:SS »).
fn seance_valable(s: &Value, profil: Option<&str>, maintenant: &str) -> Option<String> {
    let recent = rappels::secondes_entre(s["vu"].as_str()?, maintenant);
    let meme = maintenant.starts_with(s["jour"].as_str()?)
        && s["profil"].as_str().unwrap_or("") == profil.unwrap_or("")
        && (0..REPRISE_MAX).contains(&recent);
    meme.then(|| s["session"].as_str().map(str::to_string)).flatten().filter(|id| !id.is_empty())
}

/// Le dossier de travail manque : une Eve qui porte son propre nom le dit avec celui que son lanceur a nommé, qui a perdu
/// de quoi travailler ; celle d'Entrepreneur Général renvoie à son réglage.
fn dossier_introuvable() -> String {
    match std::env::var_os("EVE_DOSSIER").filter(|_| crate::reglages::nom_affiche() != "Adjointe") {
        Some(d) => format!(
            "Le dossier de travail d'Eve est introuvable ou incomplet : {} (il lui faut un CLAUDE.md et un dossier \
             scripts).",
            PathBuf::from(d).display()
        ),
        None => "Le dossier de l'Adjointe est introuvable : réglage « adjointe » de reglages.json.".into(),
    }
}

/// Les heures où Eve dit ses alertes à voix haute (de 6 h à 21 h).
fn heures_de_bureau() -> bool {
    let h: u32 = journal::heure().get(..2).and_then(|x| x.parse().ok()).unwrap_or(12);
    (6..21).contains(&h)
}

fn signature(nom: &str, entree: &Value) -> String {
    format!("{nom}|{entree}")
}

fn majuscule(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|p| p.to_uppercase().chain(c).collect()).unwrap_or_default()
}

/// Les écritures de la journée, pour « Eve, le résumé de ma journée » : le relevé ajouté à la demande.
fn actions_du_jour() -> String {
    let date = journal::date();
    let texte =
        std::fs::read_to_string(dossier_local().join("actions").join(format!("{date}.txt"))).unwrap_or_default();
    let lignes: Vec<&str> = texte.lines().filter(|l| !l.trim().is_empty()).collect();
    if lignes.is_empty() {
        return format!("\n\n[Ce qu'Eve a fait aujourd'hui, {date} : aucune écriture notée sur ce poste.]");
    }
    let debut = lignes.len().saturating_sub(150);
    format!(
        "\n\n[Ce qu'Eve a fait aujourd'hui, {date} — {} écriture(s) sur ce poste, chacune avec son heure{} :]\n{}",
        lignes.len(),
        if debut > 0 { ", les 150 dernières" } else { "" },
        lignes[debut..].join("\n")
    )
}

#[cfg(test)]
mod essais {
    use super::*;

    #[test]
    fn le_visage_prend_le_ton_du_passage_qui_se_dit() {
        use parole::Emotion::{Joie, Serieuse, Triste};
        // « [joie] Bonne nouvelle ! [serieuse] Par contre… » : la première phrase montre la joie dès qu'elle est donnée à
        // la voix ; la deuxième (le même ton) rien ; la troisième, sérieuse, attend que la deuxième ait fini, ou de
        // s'entendre elle-même.
        let mut p = Passages::default();
        p.changer(Joie);
        assert_eq!(p.phrase(Some(5)), Some(Joie));
        assert_eq!(p.phrase(Some(6)), None);
        p.changer(Serieuse);
        assert_eq!(p.phrase(Some(8)), None, "elle attend son tour");
        assert_eq!(p.voix(5, true), None);
        assert_eq!(p.voix(5, false), None, "la phrase d'avant n'a pas fini");
        assert_eq!(p.voix(6, true), None);
        assert_eq!(p.voix(6, false), Some(Serieuse), "la phrase d'avant a fini : dans le silence, avant le son");
        assert!(p.vide() && p.voix(8, true).is_none(), "montrée une fois");
        // Sa phrase s'entend sans que la fin de l'autre ait été vue : elle se montre aussi.
        let mut q = Passages::default();
        q.changer(Joie);
        q.phrase(Some(1));
        q.changer(Triste);
        q.phrase(Some(2));
        assert_eq!(q.voix(2, true), Some(Triste));
        // Deux passages d'avance : la voix qui les atteint d'un coup montre le dernier.
        let mut r = Passages::default();
        r.changer(Joie);
        r.phrase(Some(1));
        r.changer(Serieuse);
        r.phrase(Some(2));
        r.changer(Triste);
        r.phrase(Some(3));
        assert_eq!(r.voix(3, true), Some(Triste));
        // Coupée : ce qui attendait ne se montre pas ; la phrase suivante, tout de suite.
        let mut s = Passages::default();
        s.changer(Joie);
        s.phrase(Some(1));
        s.changer(Serieuse);
        s.phrase(Some(2));
        s.couper();
        assert_eq!(s.voix(1, false), None);
        s.changer(Triste);
        assert_eq!(s.phrase(Some(4)), Some(Triste));
        // En sourdine (rien ne se dit) : chaque passage, dès qu'il s'écrit ; le premier de la réponse est retenu.
        let mut t = Passages::default();
        t.changer(Joie);
        assert_eq!(t.phrase(None), Some(Joie));
        t.changer(Serieuse);
        assert_eq!(t.phrase(None), Some(Serieuse));
        assert_eq!(t.premier(), Some(Joie));
        // Sans émotion, rien.
        assert_eq!(Passages::default().phrase(Some(1)), None);
    }

    #[test]
    fn la_conversation_du_jour_reprend_dans_l_heure() {
        let s = json!({ "jour": "2026-10-25", "profil": "", "session": "abc", "vu": "2026-10-25 09:30:00" });
        assert_eq!(seance_valable(&s, None, "2026-10-25 10:00:00").as_deref(), Some("abc"));
        assert_eq!(seance_valable(&s, None, "2026-10-25 10:31:00"), None, "plus d'une heure : le cache a refroidi");
        assert_eq!(seance_valable(&s, None, "2026-10-26 09:31:00"), None, "un autre jour : une conversation neuve");
        assert_eq!(seance_valable(&s, Some("Estimateur"), "2026-10-25 10:00:00"), None, "un autre profil");
        let p = json!({ "jour": "2026-10-25", "profil": "Estimateur", "session": "def", "vu": "2026-10-25 09:30:00" });
        assert_eq!(seance_valable(&p, Some("Estimateur"), "2026-10-25 09:45:00").as_deref(), Some("def"));
    }
}
