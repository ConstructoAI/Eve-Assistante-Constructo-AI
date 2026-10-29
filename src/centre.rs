//! Les panneaux du centre de commande de la fenêtre native (le lot 6), au style du tableau HTML que l'Assistante
//! famille s'est fait (`centre-de-commande.html`, aux archives) : les échéances à gauche, le panneau
//! d'information en bas à gauche, la décision du jour en bas au centre — posés par `natif.rs` sur le réseau des
//! actions. Leurs données : ce que le spécialiste tient à jour dans son dossier de travail, `activite\*.js`, des
//! littéraux d'objets JavaScript assignés à `window.EVE_…`, que la page lit tels quels et qu'Eve lit en JSON5. Leurs
//! formats : `docs\fenetre-native.md`, § 5.5.
//!
//! Ce qui est propre à une famille — ses personnes et leurs anniversaires, l'école, la ville de la météo, la
//! cueillette — n'est jamais écrit dans le programme, comme la page le faisait : il vient de `window.EVE_FAMILLE`
//! (`activite\famille.js`), ou, sans lui, de la valeur de secours que donne la fenêtre ([`Centre::famille`]). La
//! météo donne la neige et les rafales du jour (la tempête) ; le journal d'Eve, ses outils du jour (la journée en
//! chiffres).
//!
//! L'argent n'a qu'une source quand le réglage `comptabilite` nomme la comptabilité du ménage : l'encaisse et les
//! échéances fiscales viennent alors de son tableau de bord (`tableau\donnees.json`, sa forme dans
//! `tableau\format-donnees.md`), que le même fil relit quand il change. `budget.js` et les échéances « fiscal »
//! d'`echeances.js`, ses copies, ne comptent plus — même quand le tableau manque ou ne se lit pas : l'encaisse dit
//! alors pourquoi, plutôt que de montrer une copie d'hier comme si elle était à jour.
//!
//! Chaque élément du panneau est un [`Bloc`], le `bloc` de la page (cdc:620) : un titre, des lignes de texte, un petit
//! texte ; des fonctions pures le font d'après les données, un seul peintre le dessine. Un fil relit le dossier toutes
//! les deux secondes, un fichier seulement quand il a changé, et le journal chaque minute ; la fenêtre ne touche jamais
//! au disque.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use eframe::egui::text::{LayoutJob, TextFormat};
use eframe::egui::{self, Color32, FontId, Margin, RichText, Sense, Stroke, vec2};
use serde_json::Value;

use crate::ambiance;
use crate::natif::hud;
use crate::natif::tableau::rang;
use crate::natif::theme::{self, erp, pictos};
use crate::reseau::{Destination, MAX_PERSONNES, Personne};

/// Les fichiers relus. Pas `activite.js`, le journal des outils : Eve voit chaque outil elle-même.
const FICHIERS: [&str; 9] = [
    "budget.js", "decision.js", "echeances.js", "epicerie.js", "famille.js", "outlook.js", "questions.js", "souper.js",
    "sujet.js",
];
/// Le pas de la relecture du dossier ; celui du journal d'Eve (la journée en chiffres).
const PAS: Duration = Duration::from_secs(2);
const MINUTE: Duration = Duration::from_secs(60);
/// Le panneau d'information : quatre places, dont une au moins pour ce qui n'est pas en alerte ; une page toutes les
/// 8 s (cdc:621-628).
const PLACES: usize = 4;
const PAGE: f64 = 8.0;
/// Une alerte clignote : allumée 0,6 s, éteinte 0,6 s (`clig 1.2s steps(1)`, cdc:21, 47).
const CLIGNOTEMENT: f64 = 0.6;
/// Les éléments flottent : 3 points vers le haut et retour, en 6 s (`flotte`, cdc:43-48).
const FLOTTE: f32 = 3.0;
const FLOTTEMENT: f64 = 6.0;
/// Un état d'Outlook (les courriels, l'agenda) plus vieux que ceci ne dit plus rien de juste (cdc:633, 704).
const FRAIS: f64 = 600.0;
/// La tempête (cdc:673-674) : de novembre à avril, 5 cm de neige ou des rafales de 70 km/h prévues aujourd'hui.
const NEIGE: f64 = 5.0;
const RAFALES: f64 = 70.0;
/// Les tailles de la page (cdc:9, 45-46) : le texte, 13 points sur une ligne de 1,35 ; un `<b>`, 11 points espacés de
/// 0,14 em ; un `<small>`, 10 points espacés de 0,1 em.
const TAILLE: f32 = 13.0;
const TAILLE_FORTE: f32 = 11.0;
const TAILLE_PETITE: f32 = 10.0;
const INTERLIGNE: f32 = 1.35;
/// Les couleurs de la page qui ne sont pas dans `hud` : le gris sombre des titres discrets et de ce qui est fait ; le
/// bleu des courriels et le violet du Web, ceux de leurs destinations (cdc:102-103).
const SOMBRE: Color32 = Color32::from_rgb(0x48, 0x6B, 0x85);
const BLEU: Color32 = Color32::from_rgb(0x5A, 0xA9, 0xFF);
const VIOLET: Color32 = Color32::from_rgb(0xB7, 0x8C, 0xFF);
/// Les échéances de la comptabilité, aussi courtes que celles d'`echeances.js` : deux lignes du tableau au plus.
const COURT: usize = 60;
/// Le plus gros tableau de comptabilité que lit la fenêtre : le vrai fait quelques dizaines de kilo-octets. Une lecture
/// de l'entreprise n'est pas plus grosse.
const TABLEAU_MAX: u64 = 4 * 1024 * 1024;
/// La dernière lecture de l'ERP de Constructo AI, dans le dossier local : Eve l'écrit, le tableau de bord la montre.
pub const ENTREPRISE: &str = "entreprise.json";
/// La couleur d'une personne qui ne dit pas la sienne : celle de son rang, les quatre quartiers du modèle (cdc:531)
/// d'abord.
const COULEURS: [[u8; 3]; 8] = [
    [0, 229, 255],
    [255, 122, 184],
    [255, 210, 63],
    [95, 255, 196],
    [183, 140, 255],
    [255, 159, 67],
    [124, 255, 107],
    [90, 169, 255],
];

#[derive(Default)]
pub struct Centre {
    /// Les valeurs lues, par nom (`EVE_BUDGET`, `EVE_DECISION`…).
    valeurs: Arc<Mutex<HashMap<String, Value>>>,
    /// `EVE_FAMILLE` quand `famille.js` ne le donne pas : celui que donne la fenêtre (de `reglages.json`).
    secours: Mutex<Value>,
    /// La neige (cm) et les rafales (km/h) que la météo prévoit aujourd'hui.
    meteo: Mutex<Option<(f64, f64)>>,
    /// La journée en chiffres : le jour que montre la fenêtre, et ce que le fil en a compté au journal.
    journee: Arc<Mutex<Journee>>,
    /// La comptabilité que nomme le réglage, et ce que le fil a lu de son tableau.
    comptabilite: Arc<Mutex<Comptabilite>>,
    /// La dernière lecture de l'ERP que garde le dossier local ([`ENTREPRISE`]) : ce qu'elle dit, ou pourquoi elle ne
    /// se lit pas ; `None` sans fichier, ou tant que le fil ne l'a pas lue.
    entreprise: Arc<Mutex<Option<Result<Arc<Value>, String>>>>,
    /// Réveille le fil : une comptabilité nommée se lit aussitôt. `None` sans fil.
    reveil: Option<mpsc::Sender<()>>,
    /// Combien de fois ce que lit le centre a changé (le fil le compte, et la fenêtre quand elle lui donne la famille
    /// de secours ou la comptabilité) ; le planning se refait quand il change, ou le jour.
    generation: Arc<AtomicU64>,
    /// Le dernier planning fait : son jour, la génération d'où il vient, lui.
    planning_fait: Mutex<Option<(String, u64, Arc<crate::planning::Planning>)>>,
}

/// La comptabilité que lit la fenêtre : le dossier que nomme le réglage (vide : aucune), et ce que dit son tableau — ce
/// qu'il donne, ou pourquoi il ne se lit pas ; `None` tant que le fil ne l'a pas lu.
#[derive(Default)]
struct Comptabilite {
    dossier: String,
    tableau: Option<Result<Tableau, String>>,
}

/// Ce que la fenêtre prend du tableau de la comptabilité : sa date (`misAJour`), ses échéances — de la forme de celles
/// d'`echeances.js`, de la sorte « fiscal » —, les remises de ses personnes, et combien d'éléments mal écrits sont
/// laissés de côté.
#[derive(Clone, Debug, Default, PartialEq)]
struct Tableau {
    maj: Option<String>,
    echeances: Vec<Value>,
    remises: Vec<Remise>,
    ignores: usize,
}

/// Une somme à remettre aux gouvernements : sa date, son montant.
#[derive(Clone, Debug, PartialEq)]
struct Remise {
    date: String,
    montant: f64,
}

/// Ce que le fil sait du tableau qu'il suit : le dossier ; la version (la date et la taille du fichier) que montre la
/// fenêtre, et celle qu'il a vue à son passage d'avant ; la raison qu'il a dite au journal.
#[derive(Default)]
struct Suivi {
    dossier: String,
    montree: Option<crate::reglages::Date>,
    vue: Option<crate::reglages::Date>,
    dite: Option<String>,
}

#[derive(Default)]
struct Journee {
    /// Le jour que montre le panneau (« 2026-10-23 ») : le fil compte celui-là.
    voulu: String,
    /// Le jour compté, et ce qu'il en a compté.
    compte: String,
    decompte: Decompte,
}

/// Les outils d'un jour, rangés comme la page les range (cdc:735-736) ; et les demandes que Claude a faites, et ce
/// qu'il a coûté ce jour-là (`mesures::bilan_du_jour`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Decompte {
    actions: u32,
    web: u32,
    courriels: u32,
    fichiers: u32,
    /// Les commandes, les outils de Claude Code, les serveurs MCP : « commandes et outils ».
    autres: u32,
    demandes: u32,
    cout: f64,
}

/// Les éléments du panneau d'information, dans l'ordre du tableau (cdc:622).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Element {
    Tempete,
    Cueillette,
    Courriels,
    Agenda,
    Rappels,
    Souper,
    Epicerie,
    Questions,
    Encaisse,
    Compte,
    Chiffres,
}

/// Un élément du tableau, prêt à dessiner : le `bloc` de la page (cdc:620) — un liseré, un titre, des lignes.
#[derive(Clone, Debug, PartialEq)]
struct Bloc {
    /// Le titre, un `<b>` de la page.
    titre: String,
    /// La couleur du titre et du liseré hors alerte ; `None` : le gris des titres et le cyan des liserés de la page.
    /// Une échéance prend celle de sa sorte.
    couleur: Option<Color32>,
    /// En alerte : un fond rouge, le liseré rouge, le titre rouge ; ses `<b>` clignotent.
    alerte: bool,
    lignes: Vec<Ligne>,
}

#[derive(Clone, Debug, PartialEq)]
enum Ligne {
    /// Un `<span>` : du texte, où un morceau fort est un `<b>`.
    Texte(Vec<Morceau>),
    /// Un `<small>` : petit, gris, espacé.
    Petit(String),
    /// La jauge de l'encaisse : la part du temps écoulé, une marque par remise (de 0 à 1).
    Jauge { part: f32, marques: Vec<f32> },
}

#[derive(Clone, Debug, PartialEq)]
struct Morceau {
    texte: String,
    couleur: Color32,
    /// Un `<b>` : plus petit, espacé ; il clignote quand son élément est en alerte.
    fort: bool,
    /// Barré : un article d'épicerie acheté.
    barre: bool,
}

/// La décision du jour : son texte, ce qui le suit (« AVANT CE SOIR »…), et si son échéance est dépassée.
#[derive(Debug, PartialEq)]
struct Decision {
    texte: String,
    suite: Option<String>,
    depassee: bool,
}

/// Ce qu'est un moment de la journée de la maison (la carte « Ta journée ») : un rendez-vous de l'agenda, un rappel (ou
/// une minuterie), le souper.
#[derive(Clone, Copy, Debug, PartialEq)]
enum SorteDeMoment {
    RendezVous,
    Rappel,
    Souper,
}

/// Un moment de la journée de la maison : son jour (0 aujourd'hui, 1 demain, plus : plus tard) et sa date, son heure
/// (« 16:30 » ; vide : toute la journée, ou ce soir pour le souper), ce qu'il est, son texte, son détail (vide :
/// aucun) ; dans combien de minutes il commence, pour un rendez-vous d'aujourd'hui qui commence dans l'heure (il
/// presse).
#[derive(Clone, Debug, PartialEq)]
struct Moment {
    jour: i64,
    date: String,
    heure: String,
    sorte: SorteDeMoment,
    texte: String,
    detail: String,
    bientot: Option<i64>,
}

/// Les moments que montre la carte de la journée ; au-delà, « + N autres ».
const MOMENTS: usize = 8;

/// Du texte, de la couleur du texte de la page.
fn texte(t: impl Into<String>) -> Morceau {
    teinte(t, hud::TEXTE)
}

/// Du texte d'une autre couleur (un `<span style="color:…">`).
fn teinte(t: impl Into<String>, couleur: Color32) -> Morceau {
    Morceau { texte: t.into(), couleur, fort: false, barre: false }
}

/// Un `<b>`.
fn fort(t: impl Into<String>, couleur: Color32) -> Morceau {
    Morceau { texte: t.into(), couleur, fort: true, barre: false }
}

impl Bloc {
    fn nouveau(titre: impl Into<String>, alerte: bool) -> Bloc {
        Bloc { titre: titre.into(), couleur: None, alerte, lignes: Vec::new() }
    }

    fn texte(mut self, morceaux: Vec<Morceau>) -> Bloc {
        self.lignes.push(Ligne::Texte(morceaux));
        self
    }

    fn petit(mut self, t: impl Into<String>) -> Bloc {
        self.lignes.push(Ligne::Petit(t.into()));
        self
    }
}

impl Centre {
    /// `dossier` : le dossier de travail du spécialiste ; ses panneaux sont dans `activite\`. La journée en chiffres
    /// vient du journal de cette Eve, dans son dossier local.
    pub fn nouveau(dossier: Option<PathBuf>, ctx: egui::Context) -> Centre {
        Centre::avec_journal(dossier, Some(crate::reglages::dossier_local()), ctx)
    }

    /// Le centre d'un dossier de travail (`dossier`) et d'un journal d'Eve (`journal` : le dossier de son `eve.log`,
    /// son dossier local, où sont aussi ses rappels et la dernière lecture de l'entreprise).
    pub fn avec_journal(dossier: Option<PathBuf>, journal: Option<PathBuf>, ctx: egui::Context) -> Centre {
        let mut centre = Centre::default();
        if dossier.is_some() || journal.is_some() {
            let (reveil, attente) = mpsc::channel();
            centre.reveil = Some(reveil);
            let (valeurs, journee, comptabilite, entreprise, generation) = (
                centre.valeurs.clone(),
                centre.journee.clone(),
                centre.comptabilite.clone(),
                centre.entreprise.clone(),
                centre.generation.clone(),
            );
            thread::spawn(move || {
                let activite = dossier.as_ref().map(|d| d.join("activite"));
                let travail = dossier.unwrap_or_default();
                let lus = Lus {
                    valeurs: &valeurs,
                    journee: &journee,
                    comptabilite: &comptabilite,
                    entreprise: &entreprise,
                    generation: &generation,
                };
                relire(activite.as_deref(), journal.as_deref(), &travail, lus, &attente, &ctx);
            });
        }
        centre
    }

    /// La dernière lecture de l'ERP que garde le dossier local (`entreprise.json`), telle que le fil l'a lue : ce
    /// qu'elle dit, ou pourquoi elle ne se lit pas ; `None` sans fichier. Le tableau de bord la montre.
    pub fn entreprise(&self) -> Option<Result<Arc<Value>, String>> {
        self.entreprise.lock().ok().and_then(|e| e.clone())
    }

    /// Le planning du tableau de bord ([`crate::planning`]) : le calendrier, le Gantt et le Kanban, d'après la dernière
    /// lecture de l'entreprise et les fichiers de la maison, `aujourdhui` (« AAAA-MM-JJ ») pour repère. Le point de
    /// rencontre de l'équipe du planning : ses données le remplissent, ses vues le dessinent.
    ///
    /// L'entreprise : `entreprise.json` du dossier local, tel que le fil l'a lu (`planning::de_l_entreprise` en fait
    /// les événements, les tâches, les dépendances et les tableaux) ; rien d'une Eve qui n'y est pas reliée, ni d'un
    /// fichier qui ne se lit pas. La maison, sur les mêmes fichiers que le tableau de bord
    /// ([`Centre::evenements_de_la_maison`]). Il ne se refait que quand ce que lit le centre a changé, ou le jour, et
    /// se rend partagé : la fenêtre peut le demander à chaque image, sans le recopier.
    pub fn planning(&self, aujourdhui: &str) -> Arc<crate::planning::Planning> {
        let generation = self.generation.load(Ordering::Acquire);
        if let Ok(fait) = self.planning_fait.lock()
            && let Some((jour, g, p)) = fait.as_ref()
            && jour == aujourdhui
            && *g == generation
        {
            return Arc::clone(p);
        }
        let lue = match self.entreprise() {
            Some(Ok(v)) => <crate::entreprise::Entreprise as serde::Deserialize>::deserialize(&*v).ok(),
            _ => self.donnee("EVE_ENTREPRISE").and_then(|v| serde_json::from_value(v).ok()),
        };
        let mut p = lue.map(|e| crate::planning::de_l_entreprise(&e, aujourdhui)).unwrap_or_default();
        p.evenements.extend(self.evenements_de_la_maison(aujourdhui));
        crate::planning::ranger(&mut p.evenements);
        let p = Arc::new(p);
        if let Ok(mut fait) = self.planning_fait.lock() {
            *fait = Some((aujourdhui.to_string(), generation, Arc::clone(&p)));
        }
        p
    }

    /// Les événements de la maison au calendrier du planning : les échéances (`echeances.js`, et celles de la
    /// comptabilité nommée : [`Centre::echeances_lues`]) — celles de l'école à part —, la décision du jour à son
    /// échéance, les rappels d'Eve à venir (`rappels.json` ; pas les minuteries), les anniversaires de la famille
    /// (`famille.js`, de la fenêtre du planning à l'an prochain) et l'agenda de la maison (`outlook.js`).
    fn evenements_de_la_maison(&self, aujourdhui: &str) -> Vec<crate::planning::Evenement> {
        use crate::planning::{Cote, Sorte, evenement, jour_dit};
        let maison = |sorte| (sorte, Cote::Famille);
        let mut ev = Vec::new();
        for x in self.echeances_lues().as_array().into_iter().flatten() {
            let (d, q) = (champ(x, "d"), champ(x, "q").trim());
            let (sorte, quoi) = match champ(x, "t") {
                "ecole" => (Sorte::Ecole, "École"),
                "fiscal" => (Sorte::Echeance, "Échéance fiscale"),
                "auto" => (Sorte::Echeance, "Auto"),
                "famille" => (Sorte::Echeance, "Famille"),
                _ => (Sorte::Echeance, "Échéance"),
            };
            let source = format!("l'échéance « {q} » ({}), {}", quoi.to_lowercase(), jour_dit(d));
            ev.extend(evenement((d, None, None), q.to_string(), quoi.to_string(), maison(sorte), source));
        }
        let decision = self.valeur("EVE_DECISION");
        let (texte, avant) = (champ(&decision, "texte").trim(), champ(&decision, "avant"));
        if !texte.is_empty() {
            let source = format!("la décision du jour « {texte} », à prendre avant {}", jour_dit(avant));
            let titre = format!("Décision : {}", texte.trim_end_matches(['.', ' ']));
            let detail = "Décision du jour".to_string();
            ev.extend(evenement((avant, None, None), titre, detail, maison(Sorte::Echeance), source));
        }
        for r in self.valeur("EVE_RAPPELS").as_array().into_iter().flatten() {
            if r["minuterie"].as_bool() == Some(true) {
                continue;
            }
            let (a, texte) = (champ(r, "a"), champ(r, "texte").trim());
            let (Some(jour), heure) = (a.get(..10), a.get(11..16)) else { continue };
            let quand = heure.map(|h| format!(" à {}", heure_parlee(h))).unwrap_or_default();
            let source = format!("le rappel d'Eve « {texte} », {}{quand}", jour_dit(jour));
            let detail = "Rappel d'Eve".to_string();
            ev.extend(evenement((jour, None, heure), texte.to_string(), detail, maison(Sorte::Rappel), source));
        }
        let (debut, _) = crate::planning::fenetre(aujourdhui);
        let fin = numero_du_jour(aujourdhui).map(|n| date_du_numero(n + 366)).unwrap_or_default();
        for (_, p, nom) in personnes_nommees(&self.famille()) {
            for (jour, age) in anniversaires(champ(p, "anniversaire"), &debut, &fin) {
                let ans = age.map(|a| if a > 1 { format!("{a} ans") } else { format!("{a} an") });
                let titre = format!("Anniversaire {}", de(nom));
                let quand = ans.as_deref().map(|a| format!(" ({a})")).unwrap_or_default();
                let source = format!("l'anniversaire {}{quand}, {}", de(nom), jour_dit(&jour));
                let detail = ans.unwrap_or_default();
                ev.extend(evenement((&jour, None, None), titre, detail, maison(Sorte::Anniversaire), source));
            }
        }
        for r in self.valeur("EVE_AGENDA")["rdv"].as_array().into_iter().flatten() {
            let (d, h, titre) = (champ(r, "d"), champ(r, "h").trim(), champ(r, "titre").trim());
            let heure = Some(h).filter(|h| minutes(h).is_some());
            let a = heure.map(|h| format!(" à {}", heure_parlee(h))).unwrap_or_default();
            let source = format!("le rendez-vous « {titre} » de l'agenda de la maison, {}{a}", jour_dit(d));
            let detail = "Agenda de la maison".to_string();
            ev.extend(evenement((d, None, heure), titre.to_string(), detail, maison(Sorte::RendezVous), source));
        }
        ev
    }

    /// Un centre lu d'un coup, sans fil ni fenêtre : les fichiers d'`activite\` du dossier de travail (`dossier`), la
    /// comptabilité nommée (son dossier ; vide : aucune), les rappels et la dernière lecture de l'entreprise
    /// (`entreprise.json`) du dossier local (`local`), tels qu'ils sont à l'instant. Pour le point du jour que dit le
    /// chef : les mêmes fichiers et les mêmes règles que le tableau.
    pub fn lu_maintenant(dossier: Option<&Path>, local: &Path, comptabilite: &str) -> Centre {
        let centre = Centre::default();
        if let Some(d) = dossier {
            relire_activite(&d.join("activite"), &mut HashMap::new(), &centre.valeurs);
        }
        centre.suivre_la_comptabilite(comptabilite);
        relire_la_comptabilite(&centre.comptabilite, &mut Suivi::default(), dossier.unwrap_or(Path::new("")));
        let rappels = crate::rappels::Rappels::ouvrir(local);
        let a_venir: Vec<Value> = rappels
            .a_venir(&crate::journal::date_et_heure())
            .iter()
            .map(|x| serde_json::json!({ "a": x.a, "texte": x.texte, "minuterie": x.minuterie }))
            .collect();
        if let Ok(mut v) = centre.valeurs.lock() {
            v.insert("EVE_RAPPELS".into(), Value::Array(a_venir));
            if let Ok(e) = serde_json::to_value(crate::entreprise::charger(local)) {
                v.insert("EVE_ENTREPRISE".into(), e);
            }
        }
        centre
    }

    /// Le point du jour, en phrases courtes à dire : les rendez-vous qui restent aujourd'hui, les échéances qui
    /// pressent (sous leur seuil), la décision du jour, les rappels d'aujourd'hui, l'essentiel de l'entreprise (ses
    /// factures en retard, une alerte : `entreprise::point_du_jour`), les courriels non lus — les données et les seuils
    /// du tableau. Vide s'il n'y a rien. `heure` : « 16:05 ».
    pub fn point_du_jour(&self, aujourdhui: &str, heure: &str) -> Vec<String> {
        let mut point = Vec::new();
        let agenda = self.valeur("EVE_AGENDA");
        let rdv: Vec<String> = rendez_vous(&agenda, aujourdhui, heure)
            .iter()
            .filter(|(r, _)| champ(r, "d") == aujourdhui)
            .map(|(r, _)| match champ(r, "h") {
                "" => champ(r, "titre").to_string(),
                h => format!("{} à {}", champ(r, "titre"), heure_parlee(h)),
            })
            .collect();
        if !rdv.is_empty() {
            point.push(format!("Aujourd'hui : {}.", en_liste(&rdv)));
        }
        let pressent: Vec<String> = self
            .echeances_datees(aujourdhui)
            .iter()
            .filter(|(_, b)| b.alerte)
            .map(|(_, b)| format!("{} ({})", premier_texte(b), delai_lisible(&b.titre).to_lowercase()))
            .collect();
        if !pressent.is_empty() {
            point.push(format!("À surveiller : {}.", en_liste(&pressent)));
        }
        if let Some(d) = self.decision(aujourdhui) {
            point.push(format!("La décision du jour : {}.", d.texte.trim_end_matches(['.', ' '])));
        }
        let rappels: Vec<String> = self
            .valeur("EVE_RAPPELS")
            .as_array()
            .into_iter()
            .flatten()
            .filter(|r| champ(r, "a").starts_with(aujourdhui))
            .map(|r| {
                let quand = crate::rappels::heure_dite(champ(r, "a"), aujourdhui);
                let quoi = champ(r, "texte").trim_end_matches(" est finie.").replace("La minuterie", "la minuterie");
                format!("à {quand}, {quoi}")
            })
            .collect();
        if !rappels.is_empty() {
            point.push(format!("Rappels : {}.", en_liste(&rappels)));
        }
        if let Some(e) = self.donnee("EVE_ENTREPRISE").and_then(|v| serde_json::from_value(v).ok()) {
            point.extend(crate::entreprise::point_du_jour(&e));
        }
        let non_lus: u64 = self
            .donnee("EVE_OUTLOOK")
            .and_then(|o| o["boites"].as_array().cloned())
            .into_iter()
            .flatten()
            .filter_map(|b| b["non_lus"].as_u64())
            .sum();
        if non_lus > 0 {
            point.push(format!("{non_lus} courriel{} non lu{}.", pluriel(non_lus), pluriel(non_lus)));
        }
        point
    }

    /// La comptabilité que nomme le réglage `comptabilite`, son dossier (le chef le publie ; vide : aucune). Changée,
    /// ce que la fenêtre savait de l'autre s'oublie, et le fil lit aussitôt le tableau de la nouvelle.
    pub fn suivre_la_comptabilite(&self, dossier: &str) {
        let dossier = dossier.trim();
        if let Ok(mut c) = self.comptabilite.lock()
            && c.dossier != dossier
        {
            *c = Comptabilite { dossier: dossier.to_string(), tableau: None };
            self.generation.fetch_add(1, Ordering::Release);
            if let Some(r) = &self.reveil {
                let _ = r.send(());
            }
        }
    }

    /// Les échéances : celles d'`echeances.js` ; quand une comptabilité est nommée, ses échéances fiscales à la place
    /// de celles de la copie — aucune tant que son tableau ne s'est pas lu. Les échéances du tableau, la décision du
    /// jour, le compte à rebours et l'écran de veille les prennent ici.
    pub fn echeances_lues(&self) -> Value {
        let copie = self.valeur("EVE_ECHEANCES");
        let Ok(c) = self.comptabilite.lock() else { return copie };
        if c.dossier.is_empty() {
            return copie;
        }
        let autres = copie.as_array().into_iter().flatten().filter(|e| champ(e, "t") != "fiscal").cloned();
        let fiscales = c.tableau.iter().flatten().flat_map(|t| t.echeances.iter().cloned());
        Value::Array(autres.chain(fiscales).collect())
    }

    /// L'encaisse : celle de la comptabilité quand elle est nommée (ses remises, ou pourquoi son tableau manque ; rien
    /// tant qu'il ne s'est pas lu), sinon celle de `budget.js`.
    fn encaisse(&self, aujourdhui: &str) -> Option<Bloc> {
        {
            let c = self.comptabilite.lock().ok()?;
            if !c.dossier.is_empty() {
                return match &c.tableau {
                    Some(Ok(t)) => bloc_remises(t, aujourdhui),
                    Some(Err(raison)) => Some(bloc_sans_tableau(raison)),
                    None => None,
                };
            }
        }
        bloc_encaisse(&self.valeur("EVE_BUDGET"), aujourdhui)
    }

    /// Une valeur lue (`EVE_SUJET`…), ou `Null`.
    pub fn valeur(&self, nom: &str) -> Value {
        self.valeurs.lock().ok().and_then(|v| v.get(nom).cloned()).unwrap_or(Value::Null)
    }

    /// Une valeur lue, seulement si un fichier de `activite\` l'a donnée.
    fn donnee(&self, nom: &str) -> Option<Value> {
        Some(self.valeur(nom)).filter(|v| !v.is_null())
    }

    /// Ce qui est propre à la famille : l'objet `window.EVE_FAMILLE` de `activite\famille.js`, ou, sans lui, celui de
    /// secours ; `Null` sans l'un ni l'autre. Chaque champ peut manquer :
    ///
    /// - `personnes` : de une à huit, dans l'ordre des quartiers du réseau — `nom` ; `couleur`, [r, v, b] ou
    ///   « #rrvvbb » ; `motifs`, les mots qui la nomment en plus de son nom ; `anniversaire`, « MM-JJ » ou
    ///   « AAAA-MM-JJ » (l'année la met au compte à rebours, avec son âge) ;
    /// - `ecole` : `motifs`, des mots de plus qui font penser à l'école ; `site`, à vérifier un jour de tempête ;
    ///   `conge_de`, la personne du « Prochain congé de … » ;
    /// - `meteo` : `ville`, `latitude`, `longitude`, `fuseau` ;
    /// - `cueillette` : `jours` (1 lundi … 7 dimanche ; en semaine d'office), `debut` et `fin` (« 16:40 »), `titre`,
    ///   `texte`, `note`.
    pub fn famille(&self) -> Value {
        let lue = self.valeur("EVE_FAMILLE");
        if !lue.is_null() {
            return lue;
        }
        self.secours.lock().map(|s| s.clone()).unwrap_or(Value::Null)
    }

    /// La famille de secours, quand le dossier de travail n'a pas de `famille.js` : celle de `reglages.json`.
    pub fn famille_de_secours(&self, v: Value) {
        if let Ok(mut s) = self.secours.lock()
            && *s != v
        {
            *s = v;
            self.generation.fetch_add(1, Ordering::Release);
        }
    }

    /// Les personnes de la famille, pour le réseau (leurs quartiers, « POUR … » dans sa ligne d'état) : leur nom, leur
    /// couleur (sinon celle de leur rang), les mots qui les nomment ; huit au plus.
    pub fn personnes(&self) -> Vec<Personne> {
        personnes_de(&self.famille())
    }

    /// Les mots de plus qui font penser à l'école (pour l'image de l'école du réseau).
    pub fn motifs_de_l_ecole(&self) -> Vec<String> {
        textes(&self.famille()["ecole"]["motifs"])
    }

    /// Ce que la météo prévoit aujourd'hui : la neige (cm) et les rafales (km/h) du jour ; `None` sans météo. Le
    /// panneau dit « TEMPÊTE POSSIBLE » de novembre à avril, dès 5 cm ou 70 km/h (cdc:672-677).
    pub fn tempete(&self, t: Option<(f64, f64)>) {
        if let Ok(mut m) = self.meteo.lock() {
            *m = t;
        }
    }

    /// La cueillette a-t-elle lieu à cette heure (cdc:715-721) ? Quand elle commence, la page joue une note et fait
    /// sonner la destination de la famille : la fenêtre en voit le front montant. `heure` : « 16:45 ».
    pub fn cueillette(&self, aujourdhui: &str, heure: &str) -> bool {
        self.famille().get("cueillette").is_some_and(|c| cueillette_a_lieu(c, aujourdhui, heure))
    }

    /// La destination de la famille sonne en rouge (cdc:700) : une tempête est prévue, ou c'est l'heure de la
    /// cueillette.
    pub fn alerte_famille(&self, aujourdhui: &str, heure: &str) -> bool {
        self.tempete_du_jour(aujourdhui).is_some() || self.cueillette(aujourdhui, heure)
    }

    /// Les échéances (cdc:547-563) : les quatre prochaines, de ce jour à 150 jours ; « AUJOURD'HUI », « DEMAIN » ou
    /// « DANS N J » de la couleur de leur sorte, le texte, la date. Sous le seuil de leur sorte (école 7 jours, fiscal
    /// 14, famille 3, auto 14), elles passent au rouge et leur délai clignote. Rien du tout s'il n'y en a pas.
    pub fn echeances_hud(&self, ui: &mut egui::Ui, aujourdhui: &str, temps: f64) {
        let blocs = self.echeances(aujourdhui);
        if blocs.is_empty() {
            return;
        }
        ui.spacing_mut().item_spacing.y = 0.0;
        // « ÉCHÉANCES », le `h3` de la page (cdc:49) : il est le premier des enfants, les échéances viennent après.
        etiquette(ui, capitales("ÉCHÉANCES", 10.0, SOMBRE).extra_letter_spacing(2.5));
        ui.add_space(8.0);
        for (k, b) in blocs.iter().enumerate() {
            dessiner(ui, b, k + 2, temps);
        }
    }

    /// Le panneau d'information (cdc:614-628) : les alertes d'abord, puis les autres éléments tour à tour, quatre
    /// places en tout (une au moins pour les autres), une page toutes les 8 s ; dans l'ordre de la page — la tempête,
    /// la cueillette, les courriels, l'agenda, le souper, l'épicerie, les questions, l'encaisse, le compte à rebours,
    /// la journée en chiffres. `heure` : « 20:29 ».
    pub fn panneau_hud(&self, ui: &mut egui::Ui, aujourdhui: &str, heure: &str, temps: f64) {
        let blocs = self.blocs(aujourdhui, heure, maintenant_unix());
        let tous: Vec<(Element, bool)> = blocs.iter().map(|(e, b)| (*e, b.alerte)).collect();
        ui.spacing_mut().item_spacing.y = 0.0;
        for (k, e) in montres(&tous, temps).into_iter().enumerate() {
            if let Some((_, b)) = blocs.iter().find(|(x, _)| *x == e) {
                dessiner(ui, b, k + 1, temps);
            }
        }
    }

    /// La décision du jour (cdc:799-807) : « LA DÉCISION DU JOUR », son texte dans un cadre ambre qui respire (3,2 s)
    /// et épouse son texte, centré dans la place donnée, et « ÉCHÉANCE DÉPASSÉE », « AVANT CE SOIR » ou « AVANT LE 30
    /// OCT. · DANS 7 J » ; sans décision, l'échéance la plus proche dans les quatorze jours. Rien du tout s'il n'y a ni
    /// l'une ni l'autre. À poser dans une disposition centrée (`Layout::top_down(Align::Center)`).
    pub fn decision_hud(&self, ui: &mut egui::Ui, aujourdhui: &str, temps: f64) {
        let Some(d) = self.decision(aujourdhui) else { return };
        ui.spacing_mut().item_spacing.y = 0.0;
        // Son titre, discret comme celui des échéances (`#decision small`, cdc:67).
        etiquette(ui, capitales("LA DÉCISION DU JOUR", 10.0, SOMBRE).extra_letter_spacing(3.5));
        ui.add_space(3.0);
        cadre_de_la_decision(ui, &d.texte, temps);
        if let Some(s) = d.suite {
            ui.add_space(4.0);
            etiquette(ui, capitales(&s, 11.0, hud::AMBRE));
        }
    }

    /// La décision du jour (cdc:799-807) : son texte, et ce qui le suit — « ÉCHÉANCE DÉPASSÉE », « AVANT CE SOIR » ou
    /// « AVANT LE 30 OCT. · DANS 7 J » ; sans décision, l'échéance la plus proche dans les quatorze jours
    /// (« AUJOURD'HUI », « DANS 3 J »). `None` s'il n'y a ni l'une ni l'autre. Le tableau et les cartes la montrent.
    fn decision(&self, aujourdhui: &str) -> Option<Decision> {
        let d = self.valeur("EVE_DECISION");
        let (mut texte, avant) = (champ(&d, "texte").to_string(), champ(&d, "avant"));
        let delai = jours_entre(aujourdhui, avant);
        let mut suite = delai.map(|n| match n {
            n if n < 0 => "ÉCHÉANCE DÉPASSÉE".to_string(),
            0 => "AVANT CE SOIR".to_string(),
            _ => format!("AVANT LE {} · DANS {n} J", date_courte(avant).to_uppercase()),
        });
        let mut depassee = delai.is_some_and(|n| n < 0);
        let e = self.echeances_lues();
        if texte.is_empty()
            && let Some((j, x)) = e
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|x| jours_entre(aujourdhui, champ(x, "d")).map(|j| (j, x)))
                .filter(|(j, _)| (0..=14).contains(j))
                .min_by_key(|(j, _)| *j)
        {
            texte = champ(x, "q").to_string();
            suite = Some(if j == 0 { "AUJOURD'HUI".to_string() } else { format!("DANS {j} J") });
            depassee = false;
        }
        (!texte.is_empty()).then_some(Decision { texte, suite, depassee })
    }

    // ---- La maison au tableau de bord du thème « Constructo AI » : ses cartes en listes, ses chiffres -----------

    /// Les cartes de la maison au tableau de bord, en listes ([`theme::Liste`]), chacune avec son rang (le tableau les
    /// mêle à celles de l'entreprise, le plus petit d'abord : [`rang`]) : ce qui est en alerte (la tempête, la
    /// cueillette, un courriel de l'école), la décision du jour, la journée, les échéances, les courriels, l'épicerie,
    /// les questions, l'encaisse, le compte à rebours, la journée en chiffres — seulement celles qui ont quelque chose
    /// à dire. `heure` : « 20:29 ».
    pub fn listes(&self, aujourdhui: &str, heure: &str) -> Vec<(u8, theme::Liste)> {
        let mut listes = Vec::new();
        if let Some(d) = self.decision(aujourdhui) {
            listes.push((rang::DECISION, liste_de_la_decision(&d)));
        }
        if let Some(moments) = self.journee(aujourdhui, heure) {
            listes.push((rang::JOURNEE, liste_de_la_journee(&moments)));
        }
        let echeances = self.echeances_datees(aujourdhui);
        if !echeances.is_empty() {
            listes.push((rang::ECHEANCES, liste_des_echeances(&echeances)));
        }
        let unix = maintenant_unix();
        for (e, b) in self.blocs(aujourdhui, heure, unix) {
            let r = match e {
                // L'agenda, les rappels et le souper sont dans la journée.
                Element::Agenda | Element::Rappels | Element::Souper => continue,
                _ if b.alerte => rang::ALERTES_DE_LA_MAISON,
                Element::Tempete | Element::Cueillette => rang::ALERTES_DE_LA_MAISON,
                Element::Courriels => rang::COURRIELS,
                Element::Epicerie => rang::EPICERIE,
                Element::Questions => rang::QUESTIONS,
                Element::Encaisse => rang::ENCAISSE,
                Element::Compte => rang::COMPTE,
                Element::Chiffres => rang::CHIFFRES,
            };
            // Les courriels d'un relevé frais et l'épicerie ont leurs cartes à eux ; les autres viennent de leur bloc.
            let l = match e {
                Element::Courriels if self.non_lus(unix).is_some() => {
                    self.donnee("EVE_OUTLOOK").map_or_else(|| liste_de_bloc(e, &b), |o| liste_des_courriels(&o))
                }
                Element::Epicerie => {
                    self.donnee("EVE_EPICERIE").map_or_else(|| liste_de_bloc(e, &b), |x| liste_de_l_epicerie(&x))
                }
                _ => liste_de_bloc(e, &b),
            };
            listes.push((r, l));
        }
        listes
    }

    /// Les chiffres de la maison pour les tuiles du tableau de bord, chacun sous sa clé (le tableau choisit lesquels
    /// montrer, et dans quel ordre) : « calendrier » (les rendez-vous qui restent aujourd'hui, d'un agenda frais),
    /// « courriels » (non lus, d'un relevé frais ; en alerte si l'école écrit), « rappels » (ceux d'aujourd'hui),
    /// « echeances » (celles qui pressent, sous le seuil de leur sorte ; en alerte s'il y en a), « epicerie » (les
    /// articles à acheter), « questions » (à confirmer). Seulement ceux qui ont de quoi se dire.
    pub fn chiffres(&self, aujourdhui: &str, heure: &str) -> Vec<(&'static str, theme::Chiffre)> {
        let unix = maintenant_unix();
        let chiffre = |n: usize, un: &str, plusieurs: &str, icone: char, teinte: erp::Teinte, alerte: bool| {
            let etiquette = if n > 1 { plusieurs } else { un }.to_string();
            theme::Chiffre { valeur: milliers(n as f64), etiquette, icone, teinte, alerte }
        };
        let mut c = Vec::new();
        let agenda = self.valeur("EVE_AGENDA");
        if agenda.get("t").and_then(Value::as_f64).is_some_and(|t| unix - t <= FRAIS) {
            let n = rendez_vous(&agenda, aujourdhui, heure).iter().filter(|(r, _)| champ(r, "d") == aujourdhui).count();
            let jour = "au calendrier aujourd'hui";
            c.push(("calendrier", chiffre(n, jour, jour, pictos::AGENDA, erp::VIOLET, false)));
        }
        if let Some((n, ecole)) = self.non_lus(unix) {
            let (un, plusieurs) = ("courriel non lu", "courriels non lus");
            c.push(("courriels", chiffre(n as usize, un, plusieurs, pictos::COURRIELS, erp::BLEU, ecole)));
        }
        let rappels = self.valeur("EVE_RAPPELS");
        let du_jour =
            rappels.as_array().into_iter().flatten().filter(|r| champ(r, "a").starts_with(aujourdhui)).count();
        if du_jour > 0 {
            let (un, plusieurs) = ("rappel aujourd'hui", "rappels aujourd'hui");
            c.push(("rappels", chiffre(du_jour, un, plusieurs, pictos::RAPPELS, erp::AMBRE, false)));
        }
        let echeances = self.echeances_datees(aujourdhui);
        if !echeances.is_empty() {
            let pressent = echeances.iter().filter(|(_, b)| b.alerte).count();
            let (un, plusieurs) = ("échéance qui presse", "échéances qui pressent");
            c.push(("echeances", chiffre(pressent, un, plusieurs, pictos::ECHEANCES, erp::BLEU, pressent > 0)));
        }
        if let Some(e) = self.donnee("EVE_EPICERIE") {
            let items = e.get("items").and_then(Value::as_array).cloned().unwrap_or_default();
            let a_acheter = items
                .iter()
                .filter(|a| a.as_str().is_some() || !a.get("ok").and_then(Value::as_bool).unwrap_or(false))
                .count();
            let (un, plusieurs) = ("article à acheter", "articles à acheter");
            c.push(("epicerie", chiffre(a_acheter, un, plusieurs, pictos::EPICERIE, erp::VERT, false)));
        }
        let questions = self.valeur("EVE_QUESTIONS").as_array().map_or(0, Vec::len);
        if questions > 0 {
            let (un, plusieurs) = ("question à confirmer", "questions à confirmer");
            c.push(("questions", chiffre(questions, un, plusieurs, pictos::QUESTIONS, erp::INDIGO, false)));
        }
        c
    }

    /// Les courriels non lus d'un relevé d'Outlook frais (moins de dix minutes), en tout, et si l'école en a écrit un ;
    /// `None` sans relevé frais.
    fn non_lus(&self, unix: f64) -> Option<(u64, bool)> {
        let o = self.donnee("EVE_OUTLOOK")?;
        o.get("t").and_then(Value::as_f64).filter(|t| unix - t <= FRAIS)?;
        let boites = o.get("boites").and_then(Value::as_array).cloned().unwrap_or_default();
        let n = boites.iter().filter_map(|b| b["non_lus"].as_u64()).sum();
        let ecole = boites.iter().any(|b| b["ecole"].as_bool() == Some(true) && b["non_lus"].as_u64().unwrap_or(0) > 0);
        Some((n, ecole))
    }

    /// La journée de la maison ([`moments_de_la_journee`]) : `None` si rien ne la nourrit.
    fn journee(&self, aujourdhui: &str, heure: &str) -> Option<Vec<Moment>> {
        let (agenda, rappels, souper) =
            (self.valeur("EVE_AGENDA"), self.valeur("EVE_RAPPELS"), self.donnee("EVE_SOUPER"));
        moments_de_la_journee(&agenda, &rappels, souper.as_ref(), aujourdhui, heure, maintenant_unix())
    }

    /// Les échéances à montrer, dans l'ordre (le plus proche d'abord).
    fn echeances(&self, aujourdhui: &str) -> Vec<Bloc> {
        self.echeances_datees(aujourdhui).into_iter().map(|(_, b)| b).collect()
    }

    /// Les échéances à montrer, chacune avec sa date (« 2026-10-30 ») : les cartes en font une page de calendrier.
    fn echeances_datees(&self, aujourdhui: &str) -> Vec<(String, Bloc)> {
        let e = self.echeances_lues();
        let mut venir: Vec<(i64, &Value)> = e
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|x| jours_entre(aujourdhui, champ(x, "d")).map(|j| (j, x)))
            .filter(|(j, _)| (0..=150).contains(j))
            .collect();
        venir.sort_by_key(|(j, _)| *j);
        venir
            .into_iter()
            .take(4)
            .map(|(j, x)| {
                let sorte = champ(x, "t");
                let quand = match j {
                    0 => "AUJOURD'HUI".to_string(),
                    1 => "DEMAIN".to_string(),
                    _ => format!("DANS {j} J"),
                };
                let date = champ(x, "d").to_string();
                let bloc = Bloc {
                    titre: quand,
                    couleur: Some(couleur_de_sorte(sorte)),
                    alerte: seuil(sorte).is_some_and(|s| j <= s),
                    lignes: vec![Ligne::Texte(vec![texte(champ(x, "q"))]), Ligne::Petit(date_courte(&date))],
                };
                (date, bloc)
            })
            .collect()
    }

    /// Les éléments du panneau d'information à cet instant, dans l'ordre du tableau (cdc:622), chacun seulement s'il a
    /// quelque chose à dire (la page les filtre de même). `unix` : l'heure, pour l'âge des fichiers d'Outlook. Les
    /// courriels, le souper et l'épicerie, seulement là où leur fichier existe : ailleurs, chez un client qui ne tient
    /// ni Outlook ni menus, ils ne diraient que leur absence.
    fn blocs(&self, aujourdhui: &str, heure: &str, unix: f64) -> Vec<(Element, Bloc)> {
        use Element::*;
        let famille = self.famille();
        let tous = [
            (Tempete, self.bloc_tempete(&famille, aujourdhui)),
            (Cueillette, bloc_cueillette(&famille, aujourdhui, heure)),
            (Courriels, self.donnee("EVE_OUTLOOK").map(|o| bloc_courriels(&o, unix))),
            (Agenda, bloc_agenda(&self.valeur("EVE_AGENDA"), aujourdhui, heure, unix)),
            (Rappels, bloc_rappels(&self.valeur("EVE_RAPPELS"), aujourdhui)),
            (Souper, self.donnee("EVE_SOUPER").map(|s| bloc_souper(&s, aujourdhui))),
            (Epicerie, self.donnee("EVE_EPICERIE").map(|e| bloc_epicerie(&e))),
            (Questions, bloc_questions(&self.valeur("EVE_QUESTIONS"))),
            (Encaisse, self.encaisse(aujourdhui)),
            (Compte, bloc_compte(&famille, &self.echeances_lues(), aujourdhui)),
            (Chiffres, self.bloc_chiffres(aujourdhui)),
        ];
        tous.into_iter().filter_map(|(e, b)| Some((e, b?))).collect()
    }

    /// La neige et les rafales prévues, si elles font une tempête ce jour-là.
    fn tempete_du_jour(&self, aujourdhui: &str) -> Option<(f64, f64)> {
        let (neige, rafales) = self.meteo.lock().ok().and_then(|m| *m)?;
        let mois = aujourdhui.get(5..7).and_then(|m| m.parse::<u32>().ok())?;
        let saison = mois >= 11 || (1..=4).contains(&mois);
        (saison && (neige >= NEIGE || rafales >= RAFALES)).then_some((neige, rafales))
    }

    /// La tempête (cdc:671-678), en alerte : la neige et les rafales prévues, et le site des écoles à vérifier.
    fn bloc_tempete(&self, famille: &Value, aujourdhui: &str) -> Option<Bloc> {
        let (neige, rafales) = self.tempete_du_jour(aujourdhui)?;
        let b = Bloc::nouveau("TEMPÊTE POSSIBLE", true).texte(vec![texte(format!(
            "{} cm de neige · rafales {} km/h",
            neige.round(),
            rafales.round()
        ))]);
        let site = champ(&famille["ecole"], "site").trim();
        Some(if site.is_empty() { b } else { b.petit(format!("VÉRIFIE LES ÉCOLES : {}", site.to_uppercase())) })
    }

    /// La journée en chiffres (cdc:732-739) : les outils qu'Eve a pris ce jour-là, d'après son journal. Rien avant le
    /// premier. Le fil compte le jour que la fenêtre montre ; jusqu'à ce qu'il l'ait compté (deux secondes au plus),
    /// rien.
    fn bloc_chiffres(&self, aujourdhui: &str) -> Option<Bloc> {
        let d = {
            let mut j = self.journee.lock().ok()?;
            if j.voulu != aujourdhui {
                j.voulu = aujourdhui.to_string();
            }
            (j.compte == aujourdhui).then_some(j.decompte)?
        };
        if d.actions == 0 && d.demandes == 0 {
            return None;
        }
        // Le singulier sous deux, comme le veut le français (la page écrivait toujours le pluriel).
        let mot = |n: u32, un: &'static str, plusieurs: &'static str| if n > 1 { plusieurs } else { un };
        Some(
            Bloc::nouveau("JOURNÉE EN CHIFFRES", false)
                // Les demandes et ce qu'elles ont coûté : le prix de Claude au tarif de l'API, d'après le journal.
                .texte(vec![
                    fort(d.demandes.to_string(), hud::TEXTE),
                    texte(format!(" {} · ≈ ", mot(d.demandes, "demande", "demandes"))),
                    fort(eve::mesures::dollars(d.cout), hud::JAUNE),
                ])
                .texte(vec![
                    fort(d.actions.to_string(), hud::CYAN),
                    texte(format!(" {} · ", mot(d.actions, "action", "actions"))),
                    fort(d.web.to_string(), VIOLET),
                    texte(" web · "),
                    fort(d.courriels.to_string(), BLEU),
                    texte(format!(" {}", mot(d.courriels, "courriel", "courriels"))),
                ])
                .texte(vec![
                    fort(d.fichiers.to_string(), hud::AMBRE),
                    texte(format!(" {} · ", mot(d.fichiers, "fichier", "fichiers"))),
                    fort(d.autres.to_string(), hud::VERT),
                    texte(format!(" {}", mot(d.autres, "commande ou outil", "commandes et outils"))),
                ])
                // La page disait « TOUTES SÉANCES CONFONDUES » : son journal mêlait toutes les séances de Claude Code
                // du dossier. Ici, ce sont les seules actions d'Eve, du jour.
                .petit("DEPUIS MINUIT"),
        )
    }
}

/// Ce que le panneau montre à l'instant `temps` (cdc:623-625) : les alertes d'abord, puis les autres par pages de ce
/// qui reste des quatre places — une au moins —, une page toutes les 8 s.
fn montres(tous: &[(Element, bool)], temps: f64) -> Vec<Element> {
    let alertes: Vec<Element> = tous.iter().filter(|(_, a)| *a).map(|(e, _)| *e).collect();
    let autres: Vec<Element> = tous.iter().filter(|(_, a)| !*a).map(|(e, _)| *e).collect();
    let places = PLACES.saturating_sub(alertes.len()).max(1);
    let pages = autres.len().div_ceil(places).max(1);
    let page = (temps.max(0.0) / PAGE) as usize % pages;
    let mut sortie = alertes;
    sortie.extend(autres.iter().skip(page * places).take(places));
    sortie
}

/// La cueillette (cdc:714-722), en alerte, tant qu'elle a lieu : son titre (d'office « CUEILLETTE · FERMETURE » et
/// l'heure de sa fin), son texte, sa note.
fn bloc_cueillette(famille: &Value, aujourdhui: &str, heure: &str) -> Option<Bloc> {
    let c = famille.get("cueillette")?;
    if !cueillette_a_lieu(c, aujourdhui, heure) {
        return None;
    }
    let titre = match champ(c, "titre").trim() {
        "" => match heure_dite(champ(c, "fin")) {
            Some(fin) => format!("CUEILLETTE · FERMETURE {fin}"),
            None => "CUEILLETTE".to_string(),
        },
        t => t.to_string(),
    };
    let mut b = Bloc::nouveau(titre, true);
    let (dit, note) = (champ(c, "texte").trim(), champ(c, "note").trim());
    if !dit.is_empty() {
        b = b.texte(vec![texte(dit)]);
    }
    if !note.is_empty() {
        b = b.petit(note);
    }
    Some(b)
}

/// La cueillette a lieu (cdc:716) : l'un de ses jours (1 lundi … 7 dimanche ; en semaine d'office), de son début
/// (compris) à sa fin (exclue).
fn cueillette_a_lieu(c: &Value, aujourdhui: &str, heure: &str) -> bool {
    let (Some(debut), Some(fin), Some(m), Some(jour)) =
        (minutes(champ(c, "debut")), minutes(champ(c, "fin")), minutes(heure), jour_de_la_semaine(aujourdhui))
    else {
        return false;
    };
    let jours: Vec<i64> = match c.get("jours").and_then(Value::as_array) {
        Some(a) => a.iter().filter_map(Value::as_i64).collect(),
        None => (1..=5).collect(),
    };
    jours.contains(&jour) && (debut..fin).contains(&m)
}

/// Les courriels non lus (cdc:630-638) : chaque boîte et son nombre, « ÉCOLE / GARDERIE » si un non-lu vient de
/// l'école (l'élément est alors en alerte) ; plus vieux que 10 minutes : « liaison Outlook absente ».
fn bloc_courriels(o: &Value, unix: f64) -> Bloc {
    let frais = o.get("t").and_then(Value::as_f64).is_some_and(|t| unix - t <= FRAIS);
    if !frais {
        return Bloc::nouveau("COURRIELS", false).texte(vec![texte("liaison Outlook absente")]);
    }
    let boites = o.get("boites").and_then(Value::as_array).cloned().unwrap_or_default();
    let ecole = |b: &Value| b["ecole"].as_bool() == Some(true);
    let mut bloc = Bloc::nouveau("COURRIELS NON LUS", boites.iter().any(ecole));
    for b in &boites {
        let n = b.get("non_lus").and_then(Value::as_u64).unwrap_or(0);
        let mut ligne =
            vec![texte(format!("{} · ", champ(b, "nom"))), fort(n.to_string(), if n > 0 { hud::CYAN } else { SOMBRE })];
        if ecole(b) {
            // D'un seul tenant (des espaces insécables) : en bout de ligne, il passe entier à la suivante.
            ligne.extend([texte(" · "), fort("ÉCOLE\u{a0}/\u{a0}GARDERIE", hud::ROUGE_PALE)]);
        }
        bloc = bloc.texte(ligne);
    }
    bloc
}

/// L'agenda d'aujourd'hui et de demain (cdc:702-712), quatre au plus : ceux d'aujourd'hui jusqu'à 15 min après leur
/// heure, « DEMAIN » devant les autres, « JOURNÉE » sans heure ; en alerte quand l'un commence dans l'heure. Plus vieux
/// que 10 minutes, ou sans fichier : rien.
fn bloc_agenda(a: &Value, aujourdhui: &str, heure: &str, unix: f64) -> Option<Bloc> {
    if !a.get("t").and_then(Value::as_f64).is_some_and(|t| unix - t <= FRAIS) {
        return None;
    }
    let rdv = rendez_vous(a, aujourdhui, heure);
    let bientot = rdv.iter().any(|(_, dans)| dans.is_some_and(|m| (0..=60).contains(&m)));
    let mut b = Bloc::nouveau("AGENDA · AUJOURD'HUI ET DEMAIN", bientot);
    if rdv.is_empty() {
        b = b.texte(vec![teinte("rien au calendrier", hud::GRIS)]);
    }
    for (r, _) in rdv.iter().take(4) {
        let du_jour = champ(r, "d") == aujourdhui;
        let h = champ(r, "h");
        let quand = format!("{}{}", if du_jour { "" } else { "DEMAIN " }, if h.is_empty() { "JOURNÉE" } else { h });
        let titre: String = champ(r, "titre").chars().take(42).collect();
        b = b.texte(vec![fort(quand, if du_jour { hud::CYAN } else { hud::GRIS }), texte(format!(" · {titre}"))]);
    }
    Some(b)
}

/// Le souper (cdc:640-646) : le menu du jour et ce qu'il faut sortir du congélateur, sinon de quoi en demander un.
fn bloc_souper(s: &Value, aujourdhui: &str) -> Bloc {
    let b = Bloc::nouveau("SOUPER CE SOIR", false);
    let menu = champ(s, "menu");
    if champ(s, "date") != aujourdhui || menu.is_empty() {
        return b.texte(vec![teinte("aucun menu prévu · demande-moi un menu de la semaine", hud::GRIS)]);
    }
    let b = b.texte(vec![texte(menu)]);
    match champ(s, "sortir") {
        "" => b,
        sortir => b.petit(format!("À SORTIR : {sortir}")),
    }
}

/// L'épicerie (cdc:724-730) : combien d'articles restent à acheter, les cinq premiers (☐, ou ☑ et barrés), puis
/// « + N AUTRES ».
fn bloc_epicerie(e: &Value) -> Bloc {
    let articles: Vec<(String, bool)> = e
        .get("items")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|a| match a.as_str() {
            Some(s) => (s.to_string(), false),
            None => (champ(a, "n").to_string(), a.get("ok").and_then(Value::as_bool).unwrap_or(false)),
        })
        .filter(|(n, _)| !n.is_empty())
        .collect();
    let a_acheter = articles.iter().filter(|(_, ok)| !ok).count();
    let mut b = Bloc::nouveau(format!("ÉPICERIE · {a_acheter} À ACHETER"), false);
    if articles.is_empty() {
        b = b.texte(vec![teinte("liste vide · dis-moi quoi ajouter", hud::GRIS)]);
    }
    for (n, ok) in articles.iter().take(5) {
        let article = if *ok {
            Morceau { texte: format!("☑ {n}"), couleur: SOMBRE, fort: false, barre: true }
        } else {
            texte(format!("☐ {n}"))
        };
        b = b.texte(vec![article]);
    }
    if articles.len() > 5 {
        b = b.petit(format!("+ {}", autres(articles.len() - 5)));
    }
    b
}

/// « 16:30 » → « 16 h 30 », « 09:00 » → « 9 h » : une heure telle qu'on la dit.
pub(crate) fn heure_parlee(h: &str) -> String {
    match h.split_once(':') {
        Some((h, m)) => {
            let h = h.trim_start_matches('0');
            let h = if h.is_empty() { "0" } else { h };
            if m == "00" { format!("{h} h") } else { format!("{h} h {m}") }
        }
        None => h.to_string(),
    }
}

/// Une liste dite : « a », « a et b », « a, b et c ».
pub(crate) fn en_liste(elements: &[String]) -> String {
    match elements {
        [] => String::new(),
        [seul] => seul.clone(),
        [debut @ .., dernier] => format!("{} et {dernier}", debut.join(", ")),
    }
}

/// Le texte d'un bloc (sa première ligne de texte), sans ses couleurs.
fn premier_texte(b: &Bloc) -> String {
    b.lignes
        .iter()
        .find_map(|l| if let Ligne::Texte(m) = l { Some(m.iter().map(|x| x.texte.as_str()).collect()) } else { None })
        .unwrap_or_default()
}

/// Le « s » d'un pluriel, à partir de deux.
fn pluriel(n: u64) -> &'static str {
    if n > 1 { "s" } else { "" }
}

/// Les rappels et les minuteries à venir (`rappels.json` du dossier local) : « RAPPELS · 2 À VENIR », puis l'heure et
/// le texte de chacun, trois au plus. Rien s'il n'y en a pas.
fn bloc_rappels(r: &Value, aujourdhui: &str) -> Option<Bloc> {
    let l = r.as_array().filter(|l| !l.is_empty())?;
    let mut b = Bloc::nouveau(format!("RAPPELS · {} À VENIR", l.len()), false);
    for x in l.iter().take(3) {
        let quand = crate::rappels::heure_dite(champ(x, "a"), aujourdhui);
        // Une minuterie dit sa durée : « La minuterie de 10 minutes est finie. » → « minuterie de 10 minutes ».
        let quoi = if x["minuterie"].as_bool() == Some(true) {
            champ(x, "texte").trim_end_matches(" est finie.").replace("La minuterie", "minuterie")
        } else {
            champ(x, "texte").to_string()
        };
        b = b.texte(vec![fort(quand, hud::JAUNE), texte(format!(" · {quoi}"))]);
    }
    if l.len() > 3 {
        b = b.petit(format!("+ {}", autres(l.len() - 3)));
    }
    Some(b)
}

/// La journée de la maison, dans l'ordre du temps : les rendez-vous d'aujourd'hui et de demain (un agenda frais, comme
/// ceux de sa carte du centre de commande : [`rendez_vous`]), les rappels à venir, le souper de ce soir — ou, sans
/// menu du jour, de quoi en demander un. `None` si rien ne la nourrit (ni agenda frais, ni rappel, ni fichier du
/// souper) : sa carte ne se montre pas. Un jour, ce qui dure toute la journée vient d'abord, le souper juste avant
/// 18 h. `unix` : l'heure, pour l'âge du fichier d'Outlook.
fn moments_de_la_journee(
    agenda: &Value,
    rappels: &Value,
    souper: Option<&Value>,
    aujourdhui: &str,
    heure: &str,
    unix: f64,
) -> Option<Vec<Moment>> {
    let frais = agenda.get("t").and_then(Value::as_f64).is_some_and(|t| unix - t <= FRAIS);
    let rappels = rappels.as_array().filter(|l| !l.is_empty());
    if !frais && rappels.is_none() && souper.is_none() {
        return None;
    }
    let mut moments = Vec::new();
    if frais {
        for (r, dans) in rendez_vous(agenda, aujourdhui, heure) {
            let date = champ(r, "d").to_string();
            moments.push(Moment {
                jour: jours_entre(aujourdhui, &date).unwrap_or(0),
                date,
                heure: champ(r, "h").trim().to_string(),
                sorte: SorteDeMoment::RendezVous,
                texte: champ(r, "titre").trim().to_string(),
                detail: String::new(),
                bientot: dans.filter(|m| (0..=60).contains(m)),
            });
        }
    }
    for x in rappels.into_iter().flatten() {
        let a = champ(x, "a");
        let Some((date, jour)) = a.get(..10).and_then(|d| Some((d, jours_entre(aujourdhui, d)?))) else { continue };
        // Une minuterie dit sa durée : « La minuterie de 10 minutes est finie. » → « Minuterie de 10 minutes ».
        let texte = if x["minuterie"].as_bool() == Some(true) {
            champ(x, "texte").trim_end_matches(" est finie.").replace("La minuterie", "Minuterie")
        } else {
            champ(x, "texte").trim().to_string()
        };
        let heure = a.get(11..16).unwrap_or("").to_string();
        let sorte = SorteDeMoment::Rappel;
        moments.push(Moment {
            jour: jour.max(0),
            date: date.to_string(),
            heure,
            sorte,
            texte,
            detail: String::new(),
            bientot: None,
        });
    }
    if let Some(s) = souper {
        let menu = champ(s, "menu").trim();
        let (texte, detail) = if champ(s, "date") == aujourdhui && !menu.is_empty() {
            let sortir = champ(s, "sortir").trim();
            (format!("Souper : {menu}"), if sortir.is_empty() { String::new() } else { format!("À sortir : {sortir}") })
        } else {
            ("Aucun souper prévu".to_string(), "Demande-moi un menu de la semaine".to_string())
        };
        let (date, heure) = (aujourdhui.to_string(), String::new());
        moments.push(Moment { jour: 0, date, heure, sorte: SorteDeMoment::Souper, texte, detail, bientot: None });
    }
    moments.sort_by_key(|m| {
        let heure = match (m.sorte, m.heure.as_str()) {
            (SorteDeMoment::Souper, _) => "17:59".to_string(),
            (_, "") => "00:00".to_string(),
            (_, h) => h.to_string(),
        };
        (m.jour, m.date.clone(), heure)
    });
    Some(moments)
}

/// « 1 AUTRE », « 2 AUTRES » (la page écrivait toujours le pluriel).
fn autres(n: usize) -> String {
    nombre(n, "AUTRE", "AUTRES")
}

/// Un nombre et ce qu'il compte, au singulier sous deux : « 1 remise », « 3 remises ».
fn nombre(n: usize, un: &str, plusieurs: &str) -> String {
    format!("{n} {}", if n > 1 { plusieurs } else { un })
}

/// Les questions en suspens (cdc:791-797) : les trois premières, puis combien d'autres ; rien s'il n'y en a pas.
fn bloc_questions(q: &Value) -> Option<Bloc> {
    let q: Vec<&str> = q.as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    if q.is_empty() {
        return None;
    }
    let mut b = Bloc::nouveau(format!("À CONFIRMER · {}", q.len()), false);
    for x in q.iter().take(3) {
        b = b.texte(vec![texte(*x)]);
    }
    if q.len() > 3 {
        b = b.petit(format!("+ {} · DIS-LE-MOI À L'ORAL", autres(q.len() - 3)));
    }
    Some(b)
}

/// L'encaisse (cdc:648-660) : la jauge du temps écoulé de la période (une marque par remise), l'écart cumulé estimé
/// (les mois écoulés, de 30,4 jours, fois le manque mensuel, arrondis à 50 $), la prochaine remise. Rien sans budget.
fn bloc_encaisse(b: &Value, aujourdhui: &str) -> Option<Bloc> {
    let (debut, fin) = (champ(b, "debut"), champ(b, "fin"));
    let total = jours_entre(debut, fin)?;
    // Dans combien de jours la période commence : négatif quand elle est commencée.
    let commence = jours_entre(aujourdhui, debut)?;
    let ecoule = (-commence).clamp(0, total.max(0));
    let part = |j: i64| (j as f32 / total.max(1) as f32).clamp(0.0, 1.0);
    let remises = b.get("remises").and_then(Value::as_array).cloned().unwrap_or_default();
    let marques = remises.iter().filter_map(|r| jours_entre(debut, champ(r, "d"))).map(part).collect();
    let mut bloc = Bloc::nouveau(format!("ENCAISSE · {} À {} · ESTIMATION", mois_court(debut), mois_court(fin)), false);
    bloc.lignes.push(Ligne::Jauge { part: part(ecoule), marques });
    let manque: Vec<f64> =
        b.get("manqueMensuel").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_f64).collect();
    if commence > 0 {
        bloc = bloc.texte(vec![texte(format!("début dans {commence} j"))]);
    } else if let [bas, haut] = manque[..] {
        let mois = ecoule as f64 / 30.4;
        let a_50 = |v: f64| (v / 50.0).round() * 50.0;
        let ecart = format!("écart cumulé ≈ {} $ à {} $", milliers(a_50(bas * mois)), milliers(a_50(haut * mois)));
        bloc = bloc.texte(vec![texte(ecart)]);
    }
    let prochaine = remises
        .iter()
        .filter_map(|r| jours_entre(aujourdhui, champ(r, "d")).map(|j| (j, r)))
        .filter(|(j, _)| *j >= 0)
        .min_by_key(|(j, _)| *j);
    if let Some((j, r)) = prochaine {
        let m = r.get("m").and_then(Value::as_f64).unwrap_or(0.0);
        bloc = bloc.petit(prochaine_remise(champ(r, "d"), m, j));
    }
    Some(bloc)
}

/// « PROCHAINE REMISE · 15 janv. · ≈ 3 840 $ · DANS 83 J » : la date, la somme et le délai chacun d'un seul tenant (des
/// espaces insécables), la ligne ne se coupe qu'entre eux.
fn prochaine_remise(date: &str, montant: f64, jours: i64) -> String {
    let quand = date_courte(date).replace(' ', "\u{a0}");
    format!("PROCHAINE REMISE · {quand} · ≈\u{a0}{}\u{a0}$ · DANS\u{a0}{jours}\u{a0}J", milliers(montant))
}

/// L'encaisse d'après la comptabilité : ses remises à venir — leur total jusqu'à la dernière, puis la prochaine ;
/// celles d'un même jour s'additionnent (la copie les réunissait à la main). Le titre dit la date du tableau
/// (`misAJour`) : des chiffres d'il y a un mois se voient pour tels. Ni jauge ni écart cumulé : le tableau n'a ni la
/// période du budget ni son manque mensuel. Rien sans remise à venir.
fn bloc_remises(t: &Tableau, aujourdhui: &str) -> Option<Bloc> {
    let mut par_jour: BTreeMap<i64, (&str, f64)> = BTreeMap::new();
    for r in &t.remises {
        if let Some(j) = jours_entre(aujourdhui, &r.date).filter(|j| *j >= 0) {
            par_jour.entry(j).or_insert((r.date.as_str(), 0.0)).1 += r.montant;
        }
    }
    let (&j, &(date, montant)) = par_jour.first_key_value()?;
    let titre = match &t.maj {
        Some(maj) => format!("ENCAISSE · COMPTABILITÉ DU {}", date_courte(maj).to_uppercase()),
        None => "ENCAISSE · COMPTABILITÉ".to_string(),
    };
    let mut bloc = Bloc::nouveau(titre, false);
    if let Some((_, &(derniere, _))) = par_jour.last_key_value().filter(|_| par_jour.len() > 1) {
        let total: f64 = par_jour.values().map(|(_, m)| m).sum();
        let derniere = date_courte(derniere).replace(' ', "\u{a0}");
        bloc =
            bloc.texte(vec![texte(format!("≈\u{a0}{}\u{a0}$ à remettre d'ici le\u{a0}{derniere}", milliers(total)))]);
    }
    Some(bloc.petit(prochaine_remise(date, montant, j)))
}

/// La comptabilité nommée, mais son tableau introuvable ou illisible : l'encaisse le dit, à sa place — les échéances
/// fiscales manquent aussi, et les copies ne les remplacent pas.
fn bloc_sans_tableau(raison: &str) -> Bloc {
    Bloc::nouveau("ENCAISSE · COMPTABILITÉ", false)
        .texte(vec![teinte(format!("tableau\\donnees.json {raison}"), hud::GRIS)])
        .petit("L'ENCAISSE ET LES ÉCHÉANCES FISCALES EN VIENNENT")
}

/// Ce que la fenêtre prend du tableau de la comptabilité (sa forme : `format-donnees.md`) : `misAJour` ; chaque élément
/// d'`echeances` (`date`, `quoi`) devient une échéance « fiscal », son texte raccourci comme une ligne du tableau ;
/// chaque remise des `personnes` (`echeance`, `montant`), une remise. Un élément sans date lisible, sans texte ou sans
/// montant se laisse de côté, et se compte : le reste se montre.
fn tableau_de(v: &Value) -> Tableau {
    let maj = Some(champ(v, "misAJour")).filter(|d| numero_du_jour(d).is_some()).map(str::to_string);
    let mut t = Tableau { maj, ..Tableau::default() };
    for e in v.get("echeances").and_then(Value::as_array).into_iter().flatten() {
        let (date, quoi) = (champ(e, "date"), champ(e, "quoi").trim());
        if numero_du_jour(date).is_some() && !quoi.is_empty() {
            t.echeances.push(serde_json::json!({ "d": date, "q": court(quoi, COURT), "t": "fiscal" }));
        } else {
            t.ignores += 1;
        }
    }
    let personnes = v.get("personnes").and_then(Value::as_array).into_iter().flatten();
    for r in personnes.flat_map(|p| p.get("remises").and_then(Value::as_array).into_iter().flatten()) {
        let date = champ(r, "echeance");
        match r.get("montant").and_then(Value::as_f64) {
            Some(montant) if numero_du_jour(date).is_some() => {
                t.remises.push(Remise { date: date.to_string(), montant });
            }
            _ => t.ignores += 1,
        }
    }
    t
}

/// Le tableau de la comptabilité (`donnees.json`), en JSON strict, comme le lit son propre tableau de bord ; s'il ne se
/// lit pas, pourquoi, en quelques mots (« est introuvable », « ne se lit pas (ligne 12, colonne 5) »).
fn lire_le_tableau(chemin: &Path) -> Result<Tableau, String> {
    let ouvert = |e: std::io::Error| match e.kind() {
        std::io::ErrorKind::NotFound => "est introuvable".to_string(),
        _ => "ne s'ouvre pas".to_string(),
    };
    if std::fs::metadata(chemin).map_err(ouvert)?.len() > TABLEAU_MAX {
        return Err("est trop gros pour un tableau de bord".into());
    }
    let texte = String::from_utf8(std::fs::read(chemin).map_err(ouvert)?)
        .map_err(|_| "n'est pas écrit en UTF-8".to_string())?;
    let v: Value = serde_json::from_str(texte.trim_start_matches('\u{feff}'))
        .map_err(|e| format!("ne se lit pas (ligne {}, colonne {})", e.line(), e.column()))?;
    if !v.is_object() {
        return Err("n'est pas un objet JSON".into());
    }
    Ok(tableau_de(&v))
}

/// Un texte de la comptabilité, court comme une ligne du tableau : sans ses parenthèses (des précisions), jusqu'à son
/// premier point-virgule (la suite est une autre proposition), puis `max` caractères au plus, coupé entre deux mots,
/// avec « … ». Ce qui n'en laisserait rien garde le texte entier, coupé de même.
fn court(texte: &str, max: usize) -> String {
    let net = |s: &str| {
        let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
        s.trim_end_matches(|c: char| " ,.:;·—–-".contains(c)).to_string()
    };
    let (mut sans, mut profondeur) = (String::new(), 0usize);
    for c in texte.chars() {
        match c {
            '(' => profondeur += 1,
            ')' if profondeur > 0 => profondeur -= 1,
            _ if profondeur == 0 => sans.push(c),
            _ => {}
        }
    }
    let mut t = net(sans.split(';').next().unwrap_or(""));
    if t.is_empty() {
        t = net(texte);
    }
    if t.chars().count() <= max {
        return t;
    }
    let coupe: String = t.chars().take(max.saturating_sub(1)).collect();
    let entre_deux_mots = coupe.rfind(' ').filter(|&i| coupe[..i].chars().count() >= max / 2);
    format!("{}…", net(entre_deux_mots.map_or(coupe.as_str(), |i| &coupe[..i])))
}

/// Le compte à rebours (cdc:662-669) : les anniversaires dont la famille sait l'année (« Léa · 6 ans dans 40 j », le
/// nombre de la couleur de la personne), le plus proche d'abord, et le prochain congé d'école (la première échéance
/// « ecole » à venir). Rien à compter : rien.
fn bloc_compte(famille: &Value, echeances: &Value, aujourdhui: &str) -> Option<Bloc> {
    let mut fetes: Vec<(i64, i64, &str, Color32)> = personnes_nommees(famille)
        .filter_map(|(k, p, nom)| {
            let (jours, age) = prochain_anniversaire(champ(p, "anniversaire"), aujourdhui)?;
            Some((jours, age, nom, couleur32(couleur_de(p, k))))
        })
        .collect();
    fetes.sort_by_key(|f| f.0);
    let conge = echeances
        .as_array()
        .into_iter()
        .flatten()
        .filter(|e| champ(e, "t") == "ecole")
        .filter_map(|e| jours_entre(aujourdhui, champ(e, "d")))
        .filter(|j| *j >= 0)
        .min();
    if fetes.is_empty() && conge.is_none() {
        return None;
    }
    // « 40 j », le nombre de sa couleur ; le jour même, « AUJOURD'HUI ».
    let dans = |jours: i64, couleur: Color32| -> Vec<Morceau> {
        match jours {
            0 => vec![fort("AUJOURD'HUI", couleur)],
            _ => vec![fort(jours.to_string(), couleur), texte(" j")],
        }
    };
    let mut b = Bloc::nouveau("COMPTE À REBOURS", false);
    for (jours, age, nom, couleur) in fetes {
        let ans = if age > 1 { format!("{age} ans") } else { format!("{age} an") };
        let mut ligne = vec![texte(format!("{nom} · {ans} {}", if jours == 0 { "" } else { "dans " }))];
        ligne.extend(dans(jours, couleur));
        b = b.texte(ligne);
    }
    if let Some(jours) = conge {
        let qui = champ(&famille["ecole"], "conge_de").trim();
        let personne = personnes_nommees(famille).find(|(_, _, n)| n.to_lowercase() == qui.to_lowercase());
        let couleur = personne.map_or(hud::JAUNE, |(k, p, _)| couleur32(couleur_de(p, k)));
        let quoi = if qui.is_empty() {
            "Prochain congé d'école".to_string()
        } else {
            format!("Prochain congé {}", de(qui))
        };
        let mut ligne = vec![texte(format!("{quoi} · "))];
        ligne.extend(dans(jours, couleur));
        b = b.texte(ligne);
    }
    Some(b)
}

/// Le prochain anniversaire d'une date de naissance « AAAA-MM-JJ » : dans combien de jours, et l'âge qu'il donne. Le
/// 29 février se fête le 1er mars les autres années. `None` sans l'année (« MM-JJ »), ou pour une naissance à venir.
fn prochain_anniversaire(naissance: &str, aujourdhui: &str) -> Option<(i64, i64)> {
    let mut p = naissance.trim().split('-');
    let (a, m, j) = (p.next()?, p.next()?, p.next()?);
    if a.len() != 4 || p.next().is_some() {
        return None;
    }
    let (a, m, j): (i64, u32, u32) = (a.parse().ok()?, m.parse().ok()?, j.get(..2)?.parse().ok()?);
    let annee: i64 = aujourdhui.get(..4)?.parse().ok()?;
    for annee in [annee, annee + 1] {
        let jours = jours_entre(aujourdhui, &format!("{annee:04}-{m:02}-{j:02}"))?;
        if jours >= 0 {
            return (annee - a >= 1).then_some((jours, annee - a));
        }
    }
    None
}

/// Les anniversaires d'une date de naissance (« AAAA-MM-JJ », ou « MM-JJ » sans l'année) du jour `debut` au jour `fin`,
/// compris : chaque jour, et l'âge qu'il donne quand l'année est connue (rien avant le premier). Le 29 février se fête
/// le 1er mars les autres années, comme au compte à rebours.
fn anniversaires(naissance: &str, debut: &str, fin: &str) -> Vec<(String, Option<i64>)> {
    let p: Vec<&str> = naissance.trim().split('-').collect();
    let (annee, m, j) = match p[..] {
        [a, m, j] if a.len() == 4 => (Some(a.parse::<i64>()), m, j),
        [m, j] => (None, m, j),
        _ => return Vec::new(),
    };
    let annee = match annee {
        Some(Ok(a)) => Some(a),
        Some(Err(_)) => return Vec::new(),
        None => None,
    };
    let (Ok(m), Some(Ok(j))) = (m.parse::<u32>(), j.get(..2).map(str::parse::<u32>)) else { return Vec::new() };
    let an = |d: &str| d.get(..4).and_then(|a| a.parse::<i64>().ok());
    let (Some(premier), Some(dernier)) = (an(debut), an(fin)) else { return Vec::new() };
    let mut jours = Vec::new();
    for a in premier..=dernier {
        let Some(n) = numero_du_jour(&format!("{a:04}-{m:02}-{j:02}")) else { continue };
        let jour = date_du_numero(n);
        let age = annee.map(|naissance| a - naissance);
        if jour.as_str() >= debut && jour.as_str() <= fin && age.is_none_or(|x| x >= 1) {
            jours.push((jour, age));
        }
    }
    jours
}

/// « de Léa », « d'Émile » : l'élision devant une voyelle ou un h.
fn de(nom: &str) -> String {
    let voyelle = nom.chars().next().is_some_and(|c| "aàâäeéèêëiîïoôöuùûühAÀÂÄEÉÈÊËIÎÏOÔÖUÙÛÜH".contains(c));
    if voyelle { format!("d'{nom}") } else { format!("de {nom}") }
}

/// Les personnes de la famille qui ont un nom, avec leur rang (celui de leur quartier du réseau et de leur couleur
/// d'office), huit au plus.
fn personnes_nommees(famille: &Value) -> impl Iterator<Item = (usize, &Value, &str)> {
    famille
        .get("personnes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|p| Some((p, p.get("nom")?.as_str()?.trim())).filter(|(_, n)| !n.is_empty()))
        .take(MAX_PERSONNES)
        .enumerate()
        .map(|(k, (p, n))| (k, p, n))
}

fn personnes_de(famille: &Value) -> Vec<Personne> {
    personnes_nommees(famille)
        .map(|(k, p, nom)| Personne { nom: nom.to_string(), couleur: couleur_de(p, k), motifs: textes(&p["motifs"]) })
        .collect()
}

/// La couleur d'une personne : la sienne ([r, v, b] ou « #rrvvbb »), sinon celle de son rang.
fn couleur_de(p: &Value, rang: usize) -> [u8; 3] {
    let c = p.get("couleur");
    let de_liste = c.and_then(Value::as_array).and_then(|a| {
        let x = |k: usize| a.get(k).and_then(Value::as_f64).map(|v| v.round().clamp(0.0, 255.0) as u8);
        Some([x(0)?, x(1)?, x(2)?])
    });
    let de_texte = c.and_then(Value::as_str).and_then(|s| {
        let h = s.trim().strip_prefix('#').filter(|h| h.len() == 6)?;
        let x = |k: usize| u8::from_str_radix(h.get(k..k + 2)?, 16).ok();
        Some([x(0)?, x(2)?, x(4)?])
    });
    de_liste.or(de_texte).unwrap_or(COULEURS[rang % COULEURS.len()])
}

fn couleur32([r, v, b]: [u8; 3]) -> Color32 {
    Color32::from_rgb(r, v, b)
}

/// Les chaînes non vides d'une liste.
fn textes(v: &Value) -> Vec<String> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// Les rendez-vous d'aujourd'hui (jusqu'à 15 min après leur heure) et de demain, et dans combien de minutes ils
/// commencent (aujourd'hui, à heure dite).
fn rendez_vous<'a>(a: &'a Value, aujourdhui: &str, heure: &str) -> Vec<(&'a Value, Option<i64>)> {
    let maintenant = minutes(heure);
    a.get("rdv")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|r| {
            let j = jours_entre(aujourdhui, champ(r, "d"))?;
            let dans = match (j, minutes(champ(r, "h")), maintenant) {
                (0, Some(debut), Some(m)) => Some(debut - m),
                _ => None,
            };
            let garde = match j {
                0 => dans.is_none_or(|d| d >= -15),
                1 => true,
                _ => false,
            };
            garde.then_some((r, dans))
        })
        .collect()
}

/// La journée en chiffres d'un journal d'Eve (`eve.1.log` puis `eve.log`) : les outils de la date, rangés comme la
/// page les range (cdc:736) — le Web (Chrome compris), les courriels (une commande qui ouvre Outlook), les fichiers (du
/// dossier de travail ou d'ailleurs), et le reste : les commandes, les outils de Claude Code, les serveurs MCP.
fn decompter(journal: &str, date: &str, dossier_de_travail: &Path) -> Decompte {
    let (demandes, cout) = eve::mesures::bilan_du_jour(journal, date);
    let mut d = Decompte { demandes: demandes as u32, cout, ..Decompte::default() };
    for e in crate::reseau::journee(journal, date).iter().filter(|e| !e.fin) {
        d.actions += 1;
        match crate::reseau::destination(&e.outil, &e.detail, dossier_de_travail) {
            Destination::Web | Destination::Chrome => d.web += 1,
            Destination::Outlook => d.courriels += 1,
            Destination::Travail | Destination::Dossier(_) => d.fichiers += 1,
            Destination::Commandes | Destination::Outils | Destination::Mcp(_) | Destination::Autres => d.autres += 1,
        }
    }
    d
}

/// Ce que le fil tient à jour pour la fenêtre : les valeurs d'`activite\`, la journée en chiffres, la comptabilité, la
/// dernière lecture de l'entreprise ; et combien de fois l'un d'eux a changé.
struct Lus<'a> {
    valeurs: &'a Mutex<HashMap<String, Value>>,
    journee: &'a Mutex<Journee>,
    comptabilite: &'a Mutex<Comptabilite>,
    entreprise: &'a Mutex<Option<Result<Arc<Value>, String>>>,
    generation: &'a AtomicU64,
}

/// Relit sans fin `activite\` — un fichier changé (sa date) se relit, ses affectations remplacent les anciennes —, le
/// tableau de la comptabilité nommée, la dernière lecture de l'entreprise (dans le dossier local, `journal`), et,
/// chaque minute, le journal d'Eve : la journée en chiffres du jour que montre la fenêtre (aussitôt quand ce jour
/// change). La fenêtre se redessine quand quelque chose a changé. `attente` réveille le fil avant son pas (une
/// comptabilité nommée) ; la fenêtre partie, il s'arrête.
fn relire(
    activite: Option<&Path>,
    journal: Option<&Path>,
    travail: &Path,
    lus: Lus<'_>,
    attente: &mpsc::Receiver<()>,
    ctx: &egui::Context,
) {
    let Lus { valeurs, journee, comptabilite, entreprise, generation } = lus;
    // Les rappels du dossier local (`rappels.json`, leur seule source) : ceux à venir, sous `EVE_RAPPELS`.
    let mut rappels = journal.map(crate::rappels::Rappels::ouvrir);
    let mut vus: HashMap<&str, SystemTime> = HashMap::new();
    let mut suivi = Suivi::default();
    let mut suivi_de_l_entreprise = Suivi::default();
    let mut compte_a: Option<Instant> = None;
    loop {
        let mut change = activite.is_some_and(|d| relire_activite(d, &mut vus, valeurs));
        change |= relire_la_comptabilite(comptabilite, &mut suivi, travail);
        if let Some(local) = journal {
            change |= relire_l_entreprise(&local.join(ENTREPRISE), entreprise, &mut suivi_de_l_entreprise);
        }
        if let Some(r) = &mut rappels {
            r.relire();
            let a_venir: Vec<Value> = r
                .a_venir(&crate::journal::date_et_heure())
                .iter()
                .map(|x| serde_json::json!({ "a": x.a, "texte": x.texte, "minuterie": x.minuterie }))
                .collect();
            if let Ok(mut v) = valeurs.lock()
                && v.get("EVE_RAPPELS").and_then(Value::as_array) != Some(&a_venir)
            {
                v.insert("EVE_RAPPELS".into(), Value::Array(a_venir));
                change = true;
            }
        }
        if let Some(j) = journal {
            let (voulu, compte) = journee.lock().map(|x| (x.voulu.clone(), x.compte.clone())).unwrap_or_default();
            let jour = if voulu.is_empty() { crate::journal::date() } else { voulu };
            if compte != jour || compte_a.is_none_or(|t| t.elapsed() >= MINUTE) {
                compte_a = Some(Instant::now());
                let d = decompter(&crate::journal::lire_tout(j), &jour, travail);
                if let Ok(mut x) = journee.lock()
                    && (x.compte != jour || x.decompte != d)
                {
                    x.compte = jour;
                    x.decompte = d;
                    change = true;
                }
            }
        }
        if change {
            generation.fetch_add(1, Ordering::Release);
            ctx.request_repaint();
        }
        if let Err(mpsc::RecvTimeoutError::Disconnected) = attente.recv_timeout(PAS) {
            return;
        }
    }
}

/// Un passage du fil sur la comptabilité : le tableau du dossier que nomme le réglage, relu s'il a changé (sa date ou
/// sa taille). Lu, il se montre aussitôt. S'il ne se lit pas, il attend d'être resté le même d'un passage à l'autre —
/// il s'écrit peut-être encore, et la fenêtre garde ce qu'elle montrait —, puis elle ne montre plus rien de lui et dit
/// pourquoi ; le journal le dit une fois. Vrai si ce que montre la fenêtre a changé.
fn relire_la_comptabilite(comptabilite: &Mutex<Comptabilite>, suivi: &mut Suivi, travail: &Path) -> bool {
    let (voulu, rien) = comptabilite.lock().map(|c| (c.dossier.clone(), c.tableau.is_none())).unwrap_or_default();
    if voulu != suivi.dossier {
        *suivi = Suivi { dossier: voulu.clone(), ..Suivi::default() };
    } else if rien {
        // La fenêtre n'en montre rien (elle en a nommé une autre entre deux passages, puis celle-ci) : il se relit.
        suivi.montree = None;
    }
    if voulu.is_empty() {
        return false;
    }
    let chemin = travail.join(&voulu).join("tableau").join("donnees.json");
    let version = crate::reglages::date_de(&chemin);
    if suivi.montree == Some(version) {
        return false;
    }
    let stable = suivi.vue == Some(version);
    suivi.vue = Some(version);
    let lu = match lire_le_tableau(&chemin) {
        Err(_) if !stable => return false,
        lu => lu,
    };
    suivi.montree = Some(version);
    match &lu {
        Ok(t) => {
            suivi.dite = None;
            let ignores = match t.ignores {
                0 => String::new(),
                n => format!(
                    " ; {}",
                    nombre(n, "élément mal écrit laissé de côté", "éléments mal écrits laissés de côté")
                ),
            };
            let (e, r) =
                (nombre(t.echeances.len(), "échéance", "échéances"), nombre(t.remises.len(), "remise", "remises"));
            crate::journal::ecrire(&format!("comptabilité : {} lu, {e}, {r}{ignores}", chemin.display()));
        }
        Err(raison) if suivi.dite.as_ref() != Some(raison) => {
            suivi.dite = Some(raison.clone());
            crate::journal::ecrire(&format!(
                "comptabilité : {} {raison} — la fenêtre n'en montre ni l'encaisse ni les échéances fiscales tant \
                 qu'il ne se lit pas",
                chemin.display()
            ));
        }
        Err(_) => {}
    }
    let Ok(mut c) = comptabilite.lock() else { return false };
    // La fenêtre en a nommé une autre pendant la lecture : le passage suivant lira celle-là.
    if c.dossier != voulu {
        return false;
    }
    c.tableau = Some(lu);
    true
}

/// Un passage du fil sur la dernière lecture de l'entreprise (`chemin`, [`ENTREPRISE`] du dossier local) : relue si
/// elle a changé (sa date ou sa taille), comme le tableau de la comptabilité. Lue, elle se montre aussitôt ; absente,
/// la fenêtre n'en montre plus rien. Si elle ne se lit pas, elle attend d'être restée la même d'un passage à l'autre
/// (elle s'écrit peut-être encore : la fenêtre garde ce qu'elle montrait), puis la fenêtre dit pourquoi. Vrai si ce que
/// montre la fenêtre a changé.
fn relire_l_entreprise(
    chemin: &Path,
    entreprise: &Mutex<Option<Result<Arc<Value>, String>>>,
    suivi: &mut Suivi,
) -> bool {
    let version = crate::reglages::date_de(chemin);
    if suivi.montree == Some(version) {
        return false;
    }
    let stable = suivi.vue == Some(version);
    suivi.vue = Some(version);
    let lue = match version.map(|_| lire_l_entreprise(chemin)) {
        Some(Err(_)) if !stable => return false,
        lue => lue.map(|l| l.map(Arc::new)),
    };
    suivi.montree = Some(version);
    let Ok(mut e) = entreprise.lock() else { return false };
    *e = lue;
    true
}

/// La dernière lecture de l'entreprise, en JSON strict (une marque d'ordre des octets en tête passe) ; si elle ne se
/// lit pas, pourquoi, en quelques mots, comme pour le tableau de la comptabilité.
pub fn lire_l_entreprise(chemin: &Path) -> Result<Value, String> {
    let ouvert = |e: std::io::Error| match e.kind() {
        std::io::ErrorKind::NotFound => "est introuvable".to_string(),
        _ => "ne s'ouvre pas".to_string(),
    };
    if std::fs::metadata(chemin).map_err(ouvert)?.len() > TABLEAU_MAX {
        return Err("est trop grosse pour un tableau de bord".into());
    }
    let texte = String::from_utf8(std::fs::read(chemin).map_err(ouvert)?)
        .map_err(|_| "n'est pas écrite en UTF-8".to_string())?;
    let v: Value = serde_json::from_str(texte.trim_start_matches('\u{feff}'))
        .map_err(|e| format!("ne se lit pas (ligne {}, colonne {})", e.line(), e.column()))?;
    if v.is_object() { Ok(v) } else { Err("n'est pas un objet JSON".into()) }
}

/// Relit les fichiers de `activite\` qui ont changé depuis la dernière fois (leur date) : leurs affectations remplacent
/// les anciennes. Vrai si l'un d'eux a changé.
fn relire_activite(
    dossier: &Path,
    vus: &mut HashMap<&'static str, SystemTime>,
    valeurs: &Mutex<HashMap<String, Value>>,
) -> bool {
    let mut change = false;
    for f in FICHIERS {
        let chemin = dossier.join(f);
        let Ok(date) = std::fs::metadata(&chemin).and_then(|m| m.modified()) else { continue };
        if vus.get(f) == Some(&date) {
            continue;
        }
        vus.insert(f, date);
        let Ok(octets) = std::fs::read(&chemin) else { continue };
        if let Ok(mut v) = valeurs.lock() {
            v.extend(affectations(&String::from_utf8_lossy(&octets)));
        }
        change = true;
    }
    change
}

/// Les affectations `window.EVE_X = … ;` d'un fichier : leur nom (`EVE_X`) et leur valeur, lue en JSON5. Une valeur
/// qui n'est pas un littéral (« window.EVE_LOG || [] ») s'ignore.
pub fn affectations(texte: &str) -> Vec<(String, Value)> {
    let texte = texte.trim_start_matches('\u{feff}');
    let mut sortie = Vec::new();
    let mut reste = texte;
    while let Some(i) = reste.find("window.EVE_") {
        let apres = &reste[i + "window.".len()..];
        let n = apres.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(apres.len());
        let nom = &apres[..n];
        let suite = apres[n..].trim_start();
        reste = &apres[n..];
        let Some(valeur) = suite.strip_prefix('=').filter(|s| !s.starts_with('=')) else { continue };
        let fin = fin_du_litteral(valeur);
        if let Ok(v) = json5::from_str::<Value>(valeur[..fin].trim()) {
            sortie.push((nom.to_string(), v));
        }
        reste = &valeur[fin..];
    }
    sortie
}

/// Où finit un littéral JavaScript : au premier `;` hors des chaînes, des commentaires et des crochets, sinon à la fin.
fn fin_du_litteral(s: &str) -> usize {
    let o = s.as_bytes();
    let (mut i, mut profondeur, mut chaine) = (0usize, 0i32, None::<u8>);
    while i < o.len() {
        let c = o[i];
        match chaine {
            Some(q) => {
                if c == b'\\' {
                    i += 1;
                } else if c == q {
                    chaine = None;
                }
            }
            None => match c {
                b'\'' | b'"' | b'`' => chaine = Some(c),
                b'/' if o.get(i + 1) == Some(&b'/') => {
                    while i < o.len() && o[i] != b'\n' {
                        i += 1;
                    }
                    continue;
                }
                b'/' if o.get(i + 1) == Some(&b'*') => {
                    i += 2;
                    while i + 1 < o.len() && !(o[i] == b'*' && o[i + 1] == b'/') {
                        i += 1;
                    }
                    i += 2;
                    continue;
                }
                b'[' | b'{' | b'(' => profondeur += 1,
                b']' | b'}' | b')' => profondeur -= 1,
                b';' if profondeur <= 0 => return i,
                _ => {}
            },
        }
        i += 1;
    }
    o.len()
}

/// Dessine un élément ; `rang` : son rang parmi ses voisins (son flottement). En alerte, son titre est rouge, et ses
/// `<b>` s'éteignent une demi-période sur deux (cdc:47).
fn dessiner(ui: &mut egui::Ui, b: &Bloc, rang: usize, temps: f64) {
    let eteint = b.alerte && (temps / CLIGNOTEMENT) as i64 % 2 == 1;
    let lisere = if b.alerte { hud::ROUGE } else { b.couleur.unwrap_or(hud::CYAN) };
    element_hud(ui, lisere, b.alerte, rang, temps, |ui| {
        let titre = if b.alerte { hud::ROUGE_PALE } else { b.couleur.unwrap_or(hud::GRIS) };
        etiquette(ui, mise_en_page(&[fort(b.titre.as_str(), titre)], eteint));
        for l in &b.lignes {
            match l {
                Ligne::Texte(m) => etiquette(ui, mise_en_page(m, eteint)),
                Ligne::Petit(t) => etiquette(ui, petit(t)),
                Ligne::Jauge { part, marques } => jauge(ui, *part, marques),
            }
        }
    });
}

/// Un élément du tableau (`.ech` dans la page, cdc:43-48) : un liseré à gauche de sa couleur, sur un fond de nuit ; en
/// alerte, un fond rouge. Il flotte : 3 points vers le haut et retour en 6 s, en avance de 2 s ou de 4 s selon son
/// rang parmi ses voisins (le `nth-child` de la page, compté de 1) ; sa place, elle, ne bouge pas.
fn element_hud(
    ui: &mut egui::Ui,
    couleur: Color32,
    alerte: bool,
    rang: usize,
    temps: f64,
    contenu: impl FnOnce(&mut egui::Ui),
) {
    let avance = if rang.is_multiple_of(3) {
        4.0
    } else if rang.is_multiple_of(2) {
        2.0
    } else {
        0.0
    };
    let haut = FLOTTE * flottement(temps + avance);
    ui.add_space(FLOTTE - haut);
    let fond = if alerte { hud::ROUGE.gamma_multiply(0.10) } else { hud::PANNEAU };
    let r = egui::Frame::new()
        .fill(fond)
        .inner_margin(Margin { left: 11, right: 9, top: 5, bottom: 5 })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            contenu(ui);
        })
        .response
        .rect;
    let lisere = egui::Rect::from_min_max(r.left_top(), egui::pos2(r.left() + 2.0, r.bottom()));
    ui.painter().rect_filled(lisere, 0.0, couleur);
    ui.add_space(8.0 + haut);
}

/// Le flottement d'un élément à l'instant `t`, de 0 (à sa place) à 1 (3 points plus haut).
fn flottement(t: f64) -> f32 {
    va_et_vient(t, FLOTTEMENT)
}

/// Une animation de la page qui va et revient (`flotte`, `souffle`) : de 0 à 1 à mi-période, puis à 0, chaque moitié
/// en douceur, comme son `ease-in-out`.
fn va_et_vient(t: f64, periode: f64) -> f32 {
    let p = (t / periode).rem_euclid(1.0);
    let x = (2.0 * p).min(2.0 - 2.0 * p);
    (x * x * (3.0 - 2.0 * x)) as f32
}

/// Le texte de la décision dans son cadre ambre (`#decision b`, cdc:68-71) : un bloc en ligne qui épouse son texte (6
/// points au-dessus et au-dessous, 18 sur les côtés), centré dans la place donnée, et qui ne passe à la ligne que si
/// elle manque. Son trait reste à 55 % ; sa lueur respire en 3,2 s, au-dehors (de 18 à 28 points, de 15 % à 30 %) et
/// au-dedans (de 22 à 30 points, de 18 % à 30 %).
fn cadre_de_la_decision(ui: &mut egui::Ui, texte: &str, temps: f64) {
    let marge = vec2(18.0, 6.0);
    let format = TextFormat {
        font_id: FontId::monospace(16.0),
        extra_letter_spacing: 16.0 * 0.06,
        color: hud::TEXTE_CHAUD,
        ..Default::default()
    };
    let mut job = LayoutJob::single_section(texte.to_string(), format);
    job.wrap.max_width = (ui.available_width() - 2.0 * marge.x).max(4.0 * TAILLE);
    // Les rangées centrées (le `text-align:center` de la page) : chacune autour de l'abscisse 0 de la galée.
    job.halign = egui::Align::Center;
    let galee = ui.painter().layout_job(job);
    let (r, _) = ui.allocate_exact_size(galee.size() + 2.0 * marge, Sense::hover());
    let souffle = va_et_vient(temps, 3.2);
    let entre = |a: f32, b: f32| a + (b - a) * souffle;
    let p = ui.painter();
    // Une ombre floue vaut, à son bord, la moitié de sa couleur.
    lueur(p, r, entre(18.0, 28.0), hud::AMBRE.gamma_multiply(0.5 * entre(0.15, 0.30)));
    p.rect_filled(r, 0.0, hud::AMBRE.gamma_multiply(0.08));
    lueur(p, r, -entre(22.0, 30.0), hud::AMBRE.gamma_multiply(0.5 * entre(0.18, 0.30)));
    p.rect_stroke(r, 0.0, Stroke::new(1.0, hud::AMBRE.gamma_multiply(0.55)), egui::StrokeKind::Inside);
    p.galley(egui::pos2(r.center().x, r.top() + marge.y), galee, hud::TEXTE_CHAUD);
}

/// Une lueur le long des bords d'un rectangle (le `box-shadow` sans décalage de la page) : de `couleur` au bord à rien
/// à `largeur` points, au-dehors (`largeur` positive) ou au-dedans (négative : jusqu'au milieu au plus), en deux
/// pentes, comme le flou d'une ombre. Peinte à part, elle ne passe pas sous le rectangle, comme en CSS.
fn lueur(p: &egui::Painter, r: egui::Rect, largeur: f32, couleur: Color32) {
    let largeur = if largeur < 0.0 { largeur.max(-r.width() / 2.0).max(-r.height() / 2.0) } else { largeur };
    let niveaux = [(0.0, couleur), (0.35 * largeur, couleur.gamma_multiply(0.4)), (largeur, Color32::TRANSPARENT)];
    let coins = |r: egui::Rect| [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()];
    let mut m = egui::Mesh::default();
    for paire in niveaux.windows(2) {
        let [(d1, c1), (d2, c2)] = [paire[0], paire[1]];
        let (a, b) = (coins(r.expand(d1)), coins(r.expand(d2)));
        // Un trapèze par côté : leurs coins coupés en biais se rejoignent sans se chevaucher.
        for (i, j) in [(0, 1), (1, 2), (2, 3), (3, 0)] {
            let k = m.vertices.len() as u32;
            m.colored_vertex(a[i], c1);
            m.colored_vertex(a[j], c1);
            m.colored_vertex(b[j], c2);
            m.colored_vertex(b[i], c2);
            m.add_triangle(k, k + 1, k + 2);
            m.add_triangle(k, k + 2, k + 3);
        }
    }
    p.add(egui::Shape::mesh(m));
}

/// Une ligne de texte de la page, sur sa hauteur de ligne (1,35) : ses morceaux, les forts plus petits et espacés.
/// `eteint` : les forts sont transparents (ils gardent leur place).
fn mise_en_page(morceaux: &[Morceau], eteint: bool) -> LayoutJob {
    let mut job = LayoutJob::default();
    for m in morceaux {
        let (taille, espace) = if m.fort { (TAILLE_FORTE, TAILLE_FORTE * 0.14) } else { (TAILLE, 0.0) };
        let couleur = if m.fort && eteint { Color32::TRANSPARENT } else { m.couleur };
        let format = TextFormat {
            font_id: FontId::monospace(taille),
            extra_letter_spacing: espace,
            line_height: Some(TAILLE * INTERLIGNE),
            color: couleur,
            strikethrough: if m.barre { Stroke::new(1.0, couleur) } else { Stroke::NONE },
            ..Default::default()
        };
        job.append(&m.texte, 0.0, format);
    }
    job
}

/// Un `<small>` de la page : 10 points, gris, espacés de 0,1 em.
fn petit(t: &str) -> LayoutJob {
    let format = TextFormat {
        font_id: FontId::monospace(TAILLE_PETITE),
        extra_letter_spacing: TAILLE_PETITE * 0.1,
        line_height: Some(TAILLE_PETITE * INTERLIGNE),
        color: hud::GRIS,
        ..Default::default()
    };
    LayoutJob::single_section(t.to_string(), format)
}

/// La jauge de l'encaisse (`.jauge`, cdc:56-58) : 8 points de haut, 7 au-dessus et 5 au-dessous ; le temps écoulé en
/// dégradé cyan → ambre, une marque rouge de 14 points par remise.
fn jauge(ui: &mut egui::Ui, part: f32, marques: &[f32]) {
    ui.add_space(7.0);
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 8.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(r, 0.0, hud::CYAN.gamma_multiply(0.12));
    if part > 0.0 {
        let mut plein = r;
        plein.set_width(r.width() * part);
        degrade_horizontal(p, plein, hud::CYAN, hud::AMBRE);
    }
    for m in marques {
        let x = r.left() + r.width() * m;
        let marque =
            egui::Rect::from_min_max(egui::pos2(x - 1.0, r.top() - 3.0), egui::pos2(x + 1.0, r.bottom() + 3.0));
        p.rect_filled(marque, 0.0, hud::ROUGE_PALE);
    }
    ui.add_space(5.0);
}

/// Une étiquette du tableau, qui ne prend ni le clic ni le glisser (le `pointer-events:none` de la page, cdc:12) :
/// glisser sur un panneau tourne le réseau qui est dessous.
fn etiquette(ui: &mut egui::Ui, texte: impl Into<egui::WidgetText>) {
    ui.add(egui::Label::new(texte).selectable(false));
}

/// Des petites capitales espacées, à chasse fixe (la police du tableau).
pub(crate) fn capitales(texte: &str, taille: f32, couleur: Color32) -> RichText {
    RichText::new(texte).monospace().size(taille).extra_letter_spacing(taille * 0.13).color(couleur)
}

/// La teinte et l'icône d'un élément du panneau, en carte : une couleur par élément, comme les cartes d'indicateurs du
/// tableau de bord de l'ERP.
fn habit(e: Element) -> (erp::Teinte, char) {
    match e {
        Element::Tempete => (erp::ROUGE, pictos::TEMPETE),
        Element::Cueillette => (erp::AMBRE, pictos::CUEILLETTE),
        Element::Courriels => (erp::BLEU, pictos::COURRIELS),
        Element::Agenda => (erp::VIOLET, pictos::AGENDA),
        Element::Rappels => (erp::AMBRE, pictos::RAPPELS),
        Element::Souper => (erp::ORANGE, pictos::SOUPER),
        Element::Epicerie => (erp::VERT, pictos::EPICERIE),
        Element::Questions => (erp::INDIGO, pictos::QUESTIONS),
        Element::Encaisse => (erp::SARCELLE, pictos::ENCAISSE),
        Element::Compte => (erp::JAUNE, pictos::COMPTE),
        Element::Chiffres => (erp::GRIS, pictos::CHIFFRES),
    }
}

/// Un élément du panneau en carte en liste ([`theme::Liste`]) : son titre (« ÉPICERIE · 5 À ACHETER ») se coupe en
/// titre et en pilule (« Épicerie », « 5 à acheter » : rouge en alerte) ; chacune de ses lignes devient une rangée, ses
/// morceaux forts en couleur ([`bout_clair`]) ; un petit texte qui suit une ligne devient son détail, le dernier le
/// pied de la carte ; la jauge de l'encaisse, une rangée de sa jauge.
fn liste_de_bloc(e: Element, b: &Bloc) -> theme::Liste {
    let (teinte, icone) = habit(e);
    let (titre, pilule) = match b.titre.split_once(" · ") {
        Some((t, p)) => (t, Some(p)),
        None => (b.titre.as_str(), None),
    };
    let mut l = theme::Liste::nouvelle(titre, icone, teinte);
    l.alerte = b.alerte;
    if let Some(p) = pilule {
        l.pilule = Some((p.to_string(), if b.alerte { erp::ROUGE } else { teinte }));
    }
    let derniere = b.lignes.len().saturating_sub(1);
    for (k, ligne) in b.lignes.iter().enumerate() {
        match ligne {
            Ligne::Texte(m) => {
                l.rangees.push(theme::Rangee { bouts: m.iter().map(bout_clair).collect(), ..theme::Rangee::default() });
            }
            Ligne::Petit(t) if k == derniere => l.pied = theme::casse_de_phrase(t),
            Ligne::Petit(t) => match l.rangees.last_mut() {
                Some(r) if r.detail.is_empty() => r.detail = theme::casse_de_phrase(t),
                _ => l.rangees.push(theme::Rangee::default().detail(&[theme::casse_de_phrase(t)])),
            },
            Ligne::Jauge { part, marques } => {
                let mesure = theme::Mesure::Part { part: *part, marques: marques.clone() };
                l.rangees.push(theme::Rangee { mesure: Some(mesure), ..theme::Rangee::default() });
            }
        }
    }
    l
}

/// Un morceau du tableau dans une rangée claire : sa couleur dans le thème clair ([`sur_clair`] ; l'encre du texte n'en
/// demande pas), son demi-gras, son trait ; un morceau en capitales du tableau (« DEMAIN 09:00 », « ÉCOLE / GARDERIE »)
/// en casse de phrase, comme les titres — un sigle court (« TPS ») reste.
fn bout_clair(m: &Morceau) -> theme::Bout {
    let lettres = m.texte.chars().filter(|c| c.is_alphabetic()).count();
    let capitales = lettres >= 4 && !m.texte.chars().any(char::is_lowercase);
    let texte = if capitales { theme::casse_de_phrase(&m.texte) } else { m.texte.clone() };
    let couleur = (m.couleur != hud::TEXTE).then(|| sur_clair(m.couleur));
    theme::Bout { texte, couleur, fort: m.fort, barre: m.barre }
}

/// Le mot de tête d'une rangée, dans la couleur vive d'une teinte, assombrie au besoin pour se lire sur le blanc.
fn tete(teinte: erp::Teinte) -> Color32 {
    theme::lisible(teinte.vif[1])
}

/// « 2026-10-30 » → « ven. 30 oct. » : le jour de la semaine en trois lettres, puis la date courte.
pub(crate) fn jour_court(date: &str) -> String {
    let semaine: String = ambiance::date_longue(date).chars().take(3).collect::<String>().to_lowercase();
    if numero_du_jour(date).is_some() { format!("{semaine}. {}", date_courte(date)) } else { date.to_string() }
}

/// La décision du jour en carte : son délai dans la pilule (rouge, l'échéance dépassée), son texte en demi-gras.
fn liste_de_la_decision(d: &Decision) -> theme::Liste {
    let mut l =
        theme::Liste::nouvelle("Décision du jour", pictos::DECISION, erp::AMBRE).actions(&[theme::Action::Fait]);
    let texte = theme::Bout { texte: d.texte.clone(), couleur: Some(erp::TITRE), fort: true, barre: false };
    let mut r = theme::Rangee { bouts: vec![texte], ..theme::Rangee::default() };
    // « AVANT LE 30 OCT. · DANS 7 J » : le délai dans la pilule, la date dessous — la pilule reste courte.
    if let Some(s) = &d.suite {
        let (avant, delai) = s.split_once(" · ").map_or((None, s.as_str()), |(a, b)| (Some(a), b));
        l.pilule = Some((delai_lisible(delai), if d.depassee { erp::ROUGE } else { erp::AMBRE }));
        r.detail = avant.map(delai_lisible).unwrap_or_default();
    }
    l.rangees.push(r);
    l
}

/// Les courriels non lus d'un relevé frais, en carte : chaque boîte en mot de tête (rouge si l'école ou la garderie y a
/// écrit, et le détail le dit), combien elle en a ; tous dans la pilule (rouge, si l'école a écrit).
fn liste_des_courriels(o: &Value) -> theme::Liste {
    let boites = o.get("boites").and_then(Value::as_array).cloned().unwrap_or_default();
    let ecole = |b: &Value| b["ecole"].as_bool() == Some(true);
    let total: u64 = boites.iter().filter_map(|b| b["non_lus"].as_u64()).sum();
    let alerte = boites.iter().any(ecole);
    let mut l = theme::Liste::nouvelle("Courriels non lus", pictos::COURRIELS, erp::BLEU)
        .pilule(total.to_string(), if alerte { erp::ROUGE } else { erp::BLEU });
    l.alerte = alerte;
    for b in &boites {
        let n = b["non_lus"].as_u64().unwrap_or(0);
        let combien = match n {
            0 => "aucun non lu".to_string(),
            1 => "1 non lu".to_string(),
            n => format!("{n} non lus"),
        };
        let couleur = if ecole(b) { erp::DANGER_ENCRE } else { tete(erp::BLEU) };
        let r = theme::Rangee::nouvelle(champ(b, "nom"), couleur, &combien);
        let pourquoi: &[&str] = if ecole(b) { &["dont un de l'école ou de la garderie"] } else { &[] };
        l.rangees.push(r.detail(pourquoi));
    }
    l
}

/// L'épicerie en carte, serrée : ce qui reste à acheter sur une rangée, ses articles séparés par « · » ; ce qui est
/// déjà pris dessous, en gris ; combien en reste dans la pilule.
fn liste_de_l_epicerie(e: &Value) -> theme::Liste {
    let articles: Vec<(String, bool)> = e
        .get("items")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|a| match a.as_str() {
            Some(s) => (s.trim().to_string(), false),
            None => (champ(a, "n").trim().to_string(), a.get("ok").and_then(Value::as_bool).unwrap_or(false)),
        })
        .filter(|(n, _)| !n.is_empty())
        .collect();
    let a_acheter: Vec<&str> = articles.iter().filter(|(_, ok)| !ok).map(|(n, _)| n.as_str()).collect();
    let achetes: Vec<&str> = articles.iter().filter(|(_, ok)| *ok).map(|(n, _)| n.as_str()).collect();
    let mut l = theme::Liste::nouvelle("Épicerie", pictos::EPICERIE, erp::VERT)
        .pilule(format!("{} à acheter", a_acheter.len()), erp::VERT);
    let texte = match (a_acheter.is_empty(), articles.is_empty()) {
        (_, true) => "Liste vide : dis-moi quoi ajouter".to_string(),
        (true, false) => "Tout est acheté".to_string(),
        (false, false) => a_acheter.join(" · "),
    };
    let deja = if achetes.is_empty() { String::new() } else { format!("Déjà pris : {}", achetes.join(", ")) };
    l.rangees.push(theme::Rangee::nouvelle("", erp::TEXTE, &texte).detail(&[deja]));
    l
}

/// La journée de la maison en carte, « Ta journée » : un moment par rangée, son heure en mot de tête de la couleur de
/// sa sorte (le violet d'un rendez-vous, l'ambre d'un rappel, l'orange du souper ; le rouge d'un rendez-vous qui
/// commence dans l'heure) — « 16 h 30 », « Journée », « Ce soir », « Demain 9 h », « ven. 30 oct. 10 h » —, son texte,
/// son détail. Dans la pilule, combien aujourd'hui ; ou, en rouge, dans combien de minutes commence ce qui presse (la
/// carte en alerte, comme celle de l'agenda du centre de commande).
fn liste_de_la_journee(moments: &[Moment]) -> theme::Liste {
    let mut l = theme::Liste::nouvelle("Ta journée", pictos::JOURNEE, erp::VIOLET);
    let du_jour = moments.iter().filter(|m| m.jour == 0 && m.sorte != SorteDeMoment::Souper).count();
    l.pilule = match moments.iter().find_map(|m| m.bientot) {
        Some(n) => {
            l.alerte = true;
            Some((if n <= 0 { "Maintenant".to_string() } else { format!("Dans {n} min") }, erp::ROUGE))
        }
        None => (du_jour > 0).then(|| (format!("{du_jour} aujourd'hui"), erp::VIOLET)),
    };
    if moments.is_empty() {
        l.rangees.push(theme::Rangee::nouvelle("", erp::TEXTE, "Rien au calendrier aujourd'hui ni demain"));
    }
    for m in moments.iter().take(MOMENTS) {
        let heure = if m.heure.is_empty() { String::new() } else { heure_parlee(&m.heure) };
        let quand = match (m.jour, m.sorte) {
            (0, SorteDeMoment::Souper) => "Ce soir".to_string(),
            (0, _) if heure.is_empty() => "Journée".to_string(),
            (0, _) => heure,
            (1, _) if heure.is_empty() => "Demain".to_string(),
            (1, _) => format!("Demain {heure}"),
            (_, _) => format!("{} {heure}", jour_court(&m.date)).trim_end().to_string(),
        };
        let couleur = match m.sorte {
            _ if m.bientot.is_some() => erp::DANGER_ENCRE,
            SorteDeMoment::RendezVous => tete(erp::VIOLET),
            SorteDeMoment::Rappel => tete(erp::AMBRE),
            SorteDeMoment::Souper => tete(erp::ORANGE),
        };
        let mut r = theme::Rangee::nouvelle(&quand, couleur, &m.texte);
        r.detail = m.detail.clone();
        l.rangees.push(r);
    }
    if moments.len() > MOMENTS {
        l.pied = format!("… et {}", nombre(moments.len() - MOMENTS, "autre", "autres"));
    }
    l
}

/// Les échéances en carte : une par rangée, sa date en mot de tête (« mar. 27 oct. ») de la couleur de sa sorte —
/// rouge sous son seuil, comme le tableau HTML les passe au rouge —, son texte, son délai en détail ; dans la pilule,
/// combien pressent (en rouge), sinon combien il y en a.
fn liste_des_echeances(echeances: &[(String, Bloc)]) -> theme::Liste {
    let mut l = theme::Liste::nouvelle("Échéances", pictos::ECHEANCES, erp::BLEU).actions(&[theme::Action::Fait]);
    let pressent = echeances.iter().filter(|(_, b)| b.alerte).count();
    l.pilule = Some(if pressent > 0 {
        (nombre(pressent, "qui presse", "qui pressent"), erp::ROUGE)
    } else {
        (echeances.len().to_string(), erp::BLEU)
    });
    for (date, b) in echeances {
        let couleur = if b.alerte { erp::DANGER_ENCRE } else { tete(teinte_de_sorte(b.couleur)) };
        let r = theme::Rangee::nouvelle(&jour_court(date), couleur, &premier_texte(b));
        l.rangees.push(r.detail(&[delai_lisible(&b.titre)]));
    }
    l
}

/// Un délai du tableau, pour un badge : en casse de phrase, et ses jours en toutes lettres — « DANS 5 J » → « Dans 5
/// jours », « AVANT LE 30 OCT. · DANS 7 J » → « Avant le 30 oct. · dans 7 jours », « AUJOURD'HUI » → « Aujourd'hui ».
fn delai_lisible(delai: &str) -> String {
    let phrase = theme::casse_de_phrase(delai);
    let jours =
        phrase.strip_suffix(" j").and_then(|debut| Some((debut, debut.rsplit(' ').next()?.parse::<u32>().ok()?)));
    match jours {
        Some((debut, n)) => format!("{debut} {}", if n > 1 { "jours" } else { "jour" }),
        None => phrase,
    }
}

/// La teinte d'une sorte d'échéance (sa couleur au tableau), à la manière des badges de l'ERP : l'école en jaune, le
/// fiscal en orange, l'auto en vert, la famille en bleu ; une sorte inconnue en gris.
fn teinte_de_sorte(c: Option<Color32>) -> erp::Teinte {
    match c {
        Some(hud::JAUNE) => erp::JAUNE,
        Some(hud::AMBRE) => erp::ORANGE,
        Some(hud::VERT) => erp::VERT,
        Some(hud::CYAN) => erp::BLEU,
        _ => erp::GRIS,
    }
}

/// La couleur d'un morceau du tableau dans le thème clair de l'ERP : chaque couleur du tableau a la sienne (le cyan
/// devient le bleu de Constructo AI, le gris celui de Fluent…) ; une autre (celle d'une personne) s'assombrit jusqu'à
/// se lire sur le blanc.
fn sur_clair(c: Color32) -> Color32 {
    match c {
        hud::TEXTE => erp::TEXTE,
        hud::CYAN => erp::PRIMAIRE,
        hud::GRIS => erp::TEXTE_2,
        SOMBRE => erp::TEXTE_3,
        hud::AMBRE => erp::ORANGE.encre,
        hud::VERT => erp::VERT.encre,
        hud::JAUNE => erp::JAUNE.encre,
        hud::ROUGE | hud::ROUGE_PALE => erp::ROUGE.encre,
        BLEU => erp::BLEU.encre,
        VIOLET => erp::VIOLET.encre,
        autre => theme::lisible(autre),
    }
}

/// Un dégradé de gauche (`a`) à droite (`b`).
fn degrade_horizontal(p: &egui::Painter, r: egui::Rect, a: Color32, b: Color32) {
    let mut m = egui::Mesh::default();
    m.colored_vertex(r.left_top(), a);
    m.colored_vertex(r.right_top(), b);
    m.colored_vertex(r.right_bottom(), b);
    m.colored_vertex(r.left_bottom(), a);
    m.add_triangle(0, 1, 2);
    m.add_triangle(0, 2, 3);
    p.add(egui::Shape::mesh(m));
}

fn maintenant_unix() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

fn champ<'a>(v: &'a Value, cle: &str) -> &'a str {
    v.get(cle).and_then(Value::as_str).unwrap_or("")
}

/// La couleur d'une sorte d'échéance, celle du tableau : l'école en jaune, le fiscal en orange, l'auto en vert, la
/// famille en cyan.
fn couleur_de_sorte(sorte: &str) -> Color32 {
    match sorte {
        "ecole" => hud::JAUNE,
        "fiscal" => hud::AMBRE,
        "auto" => hud::VERT,
        _ => hud::CYAN,
    }
}

/// Le seuil d'alerte d'une sorte d'échéance, en jours (cdc:549) ; une sorte inconnue n'en a pas.
fn seuil(sorte: &str) -> Option<i64> {
    match sorte {
        "ecole" => Some(7),
        "fiscal" | "auto" => Some(14),
        "famille" => Some(3),
        _ => None,
    }
}

/// « 16:40 » → 1000 minutes ; `None` pour une heure mal formée.
fn minutes(h: &str) -> Option<i64> {
    let (hh, mm) = h.trim().split_once(':')?;
    let (hh, mm) = (hh.trim().parse::<i64>().ok()?, mm.trim().get(..2)?.parse::<i64>().ok()?);
    ((0..=24).contains(&hh) && (0..60).contains(&mm)).then_some(hh * 60 + mm)
}

/// « 18:15 » → « 18 H 15 », « 17:00 » → « 17 H » : l'heure d'un titre, comme la page l'écrit.
fn heure_dite(h: &str) -> Option<String> {
    let m = minutes(h)?;
    Some(if m % 60 == 0 { format!("{} H", m / 60) } else { format!("{} H {:02}", m / 60, m % 60) })
}

/// Le numéro d'un jour (« 2026-10-23 ») depuis le 1er janvier 1970 ; `None` pour une date mal formée.
pub fn numero_du_jour(date: &str) -> Option<i64> {
    let mut p = date.trim().split('-');
    let (a, m, j): (i64, i64, i64) =
        (p.next()?.parse().ok()?, p.next()?.parse().ok()?, p.next()?.get(..2)?.parse().ok()?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&j) {
        return None;
    }
    // Le calendrier civil de Howard Hinnant, comme journal.rs.
    let a = if m <= 2 { a - 1 } else { a };
    let ere = a.div_euclid(400);
    let yoe = a - ere * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + j - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(ere * 146_097 + doe - 719_468)
}

/// Le jour de numéro `n` (l'inverse de [`numero_du_jour`]) : « 2026-10-23 ».
pub fn date_du_numero(n: i64) -> String {
    let z = n + 719_468;
    let ere = z.div_euclid(146_097);
    let doe = z - ere * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let j = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let a = yoe + ere * 400 + i64::from(m <= 2);
    format!("{a:04}-{m:02}-{j:02}")
}

/// Le jour de la semaine d'une date : 1 lundi … 7 dimanche (le 1er janvier 1970 était un jeudi).
fn jour_de_la_semaine(date: &str) -> Option<i64> {
    numero_du_jour(date).map(|n| (n + 3).rem_euclid(7) + 1)
}

/// Les jours de `de` à `a` (négatif si `a` est avant).
pub fn jours_entre(de: &str, a: &str) -> Option<i64> {
    Some(numero_du_jour(a)? - numero_du_jour(de)?)
}

const COURTS: [&str; 12] =
    ["janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.", "déc."];

/// « 2026-11-06 » → « 6 nov. » ; le premier du mois, « 1er nov. ».
pub(crate) fn date_courte(date: &str) -> String {
    let mut p = date.split('-').skip(1);
    match (p.next().and_then(|m| m.parse::<usize>().ok()), p.next().and_then(|j| j.get(..2)?.parse::<u32>().ok())) {
        (Some(m @ 1..=12), Some(j)) => format!("{j}{} {}", if j == 1 { "er" } else { "" }, COURTS[m - 1]),
        _ => date.to_string(),
    }
}

/// « 2026-11-01 » → « NOV. ».
fn mois_court(date: &str) -> String {
    match date.split('-').nth(1).and_then(|m| m.parse::<usize>().ok()) {
        Some(m @ 1..=12) => COURTS[m - 1].to_uppercase(),
        _ => String::new(),
    }
}

/// 3840 → « 3 840 » (une espace fine entre les milliers).
pub fn milliers(v: f64) -> String {
    let n = v.round() as i64;
    let t = n.abs().to_string();
    let mut s = String::new();
    for (i, c) in t.chars().enumerate() {
        if i > 0 && (t.len() - i).is_multiple_of(3) {
            s.push('\u{202f}');
        }
        s.push(c);
    }
    if n < 0 { format!("-{s}") } else { s }
}

#[cfg(test)]
mod essais {
    use serde_json::json;

    use super::*;

    impl Bloc {
        /// L'élément en clair : son titre, puis ses lignes, séparés par « | » (la jauge : « [jauge] »).
        fn en_clair(&self) -> String {
            let mut s = self.titre.clone();
            for l in &self.lignes {
                s.push_str(" | ");
                match l {
                    Ligne::Texte(m) => s.extend(m.iter().map(|m| m.texte.as_str())),
                    Ligne::Petit(t) => s.push_str(t),
                    Ligne::Jauge { .. } => s.push_str("[jauge]"),
                }
            }
            s
        }
    }

    /// Un centre sans fil, avec ces valeurs.
    fn centre(valeurs: &[(&str, Value)]) -> Centre {
        let c = Centre::default();
        c.valeurs.lock().unwrap().extend(valeurs.iter().map(|(n, v)| (n.to_string(), v.clone())));
        c
    }

    /// L'élément `e` du panneau, en clair (« » s'il n'y est pas).
    fn en_clair(c: &Centre, e: Element, aujourdhui: &str, heure: &str) -> String {
        c.blocs(aujourdhui, heure, 1_000_000.0)
            .iter()
            .find(|(x, _)| *x == e)
            .map_or(String::new(), |(_, b)| b.en_clair())
    }

    #[test]
    fn les_affectations_se_lisent_en_json5() {
        let budget = [
            "// Une période inventée : ses ESTIMATIONS.",
            "window.EVE_BUDGET = {",
            "  debut:'2026-09-01', fin:'2027-02-28',",
            "  manqueMensuel:[120, 240],",
            "  remises:[ { d:'2026-12-10', m:1500, q:'Une remise (si salaire)' }, ]",
            "};",
        ];
        let v = affectations(&budget.join("\n"));
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].0, "EVE_BUDGET");
        assert_eq!(v[0].1["remises"][0]["m"], 1500);
        // Deux affectations dans un fichier, du vrai JSON précédé d'une marque d'ordre des octets.
        let outlook = "\u{feff}window.EVE_OUTLOOK={\"t\":1,\"boites\":[]};\nwindow.EVE_AGENDA={\"t\":1,\"rdv\":[]};\n";
        let v = affectations(outlook);
        assert_eq!(v.iter().map(|x| x.0.as_str()).collect::<Vec<_>>(), ["EVE_OUTLOOK", "EVE_AGENDA"]);
        // Une chaîne qui porte un point-virgule ou une apostrophe échappée ; un commentaire dans le littéral.
        let q = "window.EVE_QUESTIONS = [\n  'Nom de l\\'enseignante ; et sa classe', // la première\n  'Budget'\n];";
        assert_eq!(affectations(q)[0].1, serde_json::json!(["Nom de l'enseignante ; et sa classe", "Budget"]));
        // Ce qui n'est pas un littéral s'ignore.
        assert!(affectations("window.EVE_LOG = window.EVE_LOG || [];\nEVE_LOG.push({\"id\":1});").is_empty());
    }

    #[test]
    fn les_jours_se_comptent() {
        assert_eq!(numero_du_jour("1970-01-01"), Some(0));
        assert_eq!(jours_entre("2026-10-23", "2026-10-31"), Some(8));
        assert_eq!(jours_entre("2026-10-23", "2027-01-15"), Some(84));
        assert_eq!(jours_entre("2026-12-31", "2027-01-01"), Some(1));
        assert_eq!(jours_entre("2028-02-28", "2028-03-01"), Some(2), "2028 est bissextile");
        assert_eq!(jours_entre("2026-10-23", "pas une date"), None);
        assert_eq!(date_courte("2026-11-06"), "6 nov.");
        assert_eq!(mois_court("2026-11-01"), "NOV.");
        assert_eq!(milliers(3840.0), "3\u{202f}840");
        // Le numéro et sa date, dans les deux sens, jusqu'aux années bissextiles.
        for d in ["1970-01-01", "1999-12-31", "2000-02-29", "2026-10-24", "2028-02-29", "2100-03-01"] {
            assert_eq!(date_du_numero(numero_du_jour(d).unwrap()), d);
        }
        assert_eq!(date_du_numero(-1), "1969-12-31");
        // Le 1er janvier 1970 était un jeudi, le 1er janvier 2000 un samedi, le 29 février 2024 un jeudi.
        assert_eq!(jour_de_la_semaine("1970-01-01"), Some(4));
        assert_eq!(jour_de_la_semaine("2000-01-01"), Some(6));
        assert_eq!(jour_de_la_semaine("2024-02-29"), Some(4));
        assert_eq!(jour_de_la_semaine("2026-10-25"), Some(7), "un dimanche");
        assert_eq!(jour_de_la_semaine("2026-10-26"), Some(1), "un lundi");
        assert_eq!(minutes("16:40"), Some(1000));
        assert_eq!(minutes(" 9:05 "), Some(545));
        assert_eq!(minutes("17h10"), None);
        assert_eq!(minutes("25:00"), None);
        assert_eq!(heure_dite("18:15").as_deref(), Some("18 H 15"));
        assert_eq!(heure_dite("17:00").as_deref(), Some("17 H"));
        assert_eq!(heure_dite("09:05").as_deref(), Some("9 H 05"));
    }

    #[test]
    fn le_panneau_montre_les_alertes_puis_tourne() {
        use Element::*;
        let calme = [
            (Courriels, false),
            (Agenda, false),
            (Souper, false),
            (Epicerie, false),
            (Questions, false),
            (Encaisse, false),
        ];
        assert_eq!(montres(&calme, 0.0), [Courriels, Agenda, Souper, Epicerie]);
        assert_eq!(montres(&calme, 8.5), [Questions, Encaisse], "la deuxième page, 8 s plus tard");
        assert_eq!(montres(&calme, 16.5), [Courriels, Agenda, Souper, Epicerie], "et l'on recommence");
        // Une alerte prend une place d'office ; les trois autres tournent.
        let mut alerte = calme;
        alerte[1].1 = true;
        assert_eq!(montres(&alerte, 0.0), [Agenda, Courriels, Souper, Epicerie]);
        assert_eq!(montres(&alerte, 8.5), [Agenda, Questions, Encaisse]);
        // Quatre alertes : les autres gardent une place, à tour de rôle (cdc:624).
        let orage =
            [(Tempete, true), (Cueillette, true), (Courriels, true), (Agenda, true), (Souper, false), (Compte, false)];
        assert_eq!(montres(&orage, 0.0), [Tempete, Cueillette, Courriels, Agenda, Souper]);
        assert_eq!(montres(&orage, 8.5), [Tempete, Cueillette, Courriels, Agenda, Compte]);
        assert_eq!(montres(&[], 3.0), []);
    }

    #[test]
    fn le_panneau_suit_l_ordre_de_la_page_et_tait_ce_qui_n_a_rien_a_dire() {
        use Element::*;
        let (auj, unix) = ("2026-10-26", 1_000_000.0);
        // Sans leurs fichiers, les courriels, le souper et l'épicerie se taisent, comme le reste : un agenda trop
        // vieux, des questions vides, pas de budget, rien à compter.
        let vieux = ("EVE_AGENDA", json!({ "t": unix - 601.0, "rdv": [{ "d": auj, "h": "", "titre": "Vieux" }] }));
        let c = centre(&[vieux.clone(), ("EVE_QUESTIONS", json!([]))]);
        assert!(c.blocs(auj, "10:00", unix).is_empty());
        // Avec leurs fichiers, même vides ou trop vieux, ils disent qu'ils n'ont rien.
        let c = centre(&[
            vieux,
            ("EVE_QUESTIONS", json!([])),
            ("EVE_OUTLOOK", json!({ "t": unix - 601.0, "boites": [] })),
            ("EVE_SOUPER", json!({})),
            ("EVE_EPICERIE", json!({ "items": [] })),
        ]);
        let elements: Vec<Element> = c.blocs(auj, "10:00", unix).into_iter().map(|(e, _)| e).collect();
        assert_eq!(elements, [Courriels, Souper, Epicerie]);
        assert_eq!(en_clair(&c, Courriels, auj, "10:00"), "COURRIELS | liaison Outlook absente");
        assert_eq!(
            en_clair(&c, Souper, auj, "10:00"),
            "SOUPER CE SOIR | aucun menu prévu · demande-moi un menu de la semaine"
        );
        assert_eq!(en_clair(&c, Epicerie, auj, "10:00"), "ÉPICERIE · 0 À ACHETER | liste vide · dis-moi quoi ajouter");
        // Tout ce qui a quelque chose à dire, dans l'ordre de la page, quel que soit l'ordre des fichiers.
        let c = centre(&[
            ("EVE_ECHEANCES", json!([{ "d": "2026-11-02", "q": "Congé", "t": "ecole" }])),
            ("EVE_BUDGET", json!({ "debut": "2026-10-01", "fin": "2027-01-31", "manqueMensuel": [100, 200] })),
            ("EVE_QUESTIONS", json!(["Une question ?"])),
            ("EVE_AGENDA", json!({ "t": unix, "rdv": [] })),
            ("EVE_OUTLOOK", json!({ "t": unix, "boites": [] })),
            ("EVE_SOUPER", json!({})),
            ("EVE_EPICERIE", json!({})),
        ]);
        c.tempete(Some((20.0, 10.0)));
        let elements: Vec<Element> = c.blocs("2026-11-02", "10:00", unix).into_iter().map(|(e, _)| e).collect();
        assert_eq!(elements, [Tempete, Courriels, Agenda, Souper, Epicerie, Questions, Encaisse, Compte]);
    }

    #[test]
    fn les_courriels_et_l_agenda_comme_la_page() {
        let (auj, unix) = ("2026-10-26", 1_000_000.0);
        let c = centre(&[
            (
                "EVE_OUTLOOK",
                json!({ "t": unix - 30.0, "boites": [
                    { "nom": "maison@exemple.ca", "non_lus": 2, "ecole": true },
                    { "nom": "bureau@exemple.ca", "non_lus": 0, "ecole": false }
                ] }),
            ),
            (
                "EVE_AGENDA",
                json!({ "t": unix - 30.0, "rdv": [
                    { "d": auj, "h": "09:50", "titre": "Commencé il y a dix minutes" },
                    { "d": auj, "h": "11:30", "titre": "Dans une heure et demie" },
                    { "d": auj, "h": "", "titre": "Toute la journée" },
                    { "d": "2026-10-27", "h": "08:15", "titre": "Demain matin" }
                ] }),
            ),
        ]);
        let blocs = c.blocs(auj, "10:00", unix);
        let (_, courriels) = blocs.iter().find(|(e, _)| *e == Element::Courriels).unwrap();
        assert!(courriels.alerte, "un non-lu de l'école");
        assert_eq!(
            courriels.en_clair(),
            "COURRIELS NON LUS | maison@exemple.ca · 2 · ÉCOLE\u{a0}/\u{a0}GARDERIE | bureau@exemple.ca · 0"
        );
        // Un compte nul en gris sombre ; « ÉCOLE / GARDERIE » en rouge pâle, et fort (il clignote en alerte).
        assert_eq!(courriels.lignes[1], Ligne::Texte(vec![texte("bureau@exemple.ca · "), fort("0", SOMBRE)]));
        let (_, agenda) = blocs.iter().find(|(e, _)| *e == Element::Agenda).unwrap();
        assert!(!agenda.alerte, "commencé il y a dix minutes, ou dans une heure et demie : pas d'alerte");
        assert_eq!(
            agenda.en_clair(),
            "AGENDA · AUJOURD'HUI ET DEMAIN | 09:50 · Commencé il y a dix minutes | 11:30 · Dans une heure et demie | \
             JOURNÉE · Toute la journée | DEMAIN 08:15 · Demain matin"
        );
        assert_eq!(agenda.lignes[3], Ligne::Texte(vec![fort("DEMAIN 08:15", hud::GRIS), texte(" · Demain matin")]));
        // Dans l'heure (de 0 à 60 minutes) : l'alerte.
        assert!(c.blocs(auj, "10:30", unix).iter().any(|(e, b)| *e == Element::Agenda && b.alerte));
        // Plus vieux que dix minutes : la liaison est absente, et l'agenda se tait.
        let blocs = c.blocs(auj, "10:00", unix + 700.0);
        assert!(blocs.iter().all(|(e, _)| *e != Element::Agenda));
        assert!(
            blocs
                .iter()
                .any(|(e, b)| *e == Element::Courriels && b.en_clair() == "COURRIELS | liaison Outlook absente")
        );
    }

    #[test]
    fn le_souper_l_epicerie_et_les_questions() {
        let auj = "2026-10-26";
        let c = centre(&[
            ("EVE_SOUPER", json!({ "date": auj, "menu": "Soupe aux pois", "sortir": "le jambon" })),
            (
                "EVE_EPICERIE",
                json!({ "items": [
                    { "n": "farine", "ok": false }, { "n": "œufs", "ok": true }, "beurre", { "n": "sel" },
                    { "n": "thé", "ok": true }, { "n": "miel" }, { "n": "" }
                ] }),
            ),
            ("EVE_QUESTIONS", json!(["Une", "Deux", "Trois", "Quatre", "Cinq"])),
        ]);
        assert_eq!(
            en_clair(&c, Element::Souper, auj, "10:00"),
            "SOUPER CE SOIR | Soupe aux pois | À SORTIR : le jambon"
        );
        assert_eq!(
            en_clair(&c, Element::Epicerie, auj, "10:00"),
            "ÉPICERIE · 4 À ACHETER | ☐ farine | ☑ œufs | ☐ beurre | ☐ sel | ☑ thé | + 1 AUTRE"
        );
        let blocs = c.blocs(auj, "10:00", 0.0);
        let (_, epicerie) = blocs.iter().find(|(e, _)| *e == Element::Epicerie).unwrap();
        assert_eq!(
            epicerie.lignes[1],
            Ligne::Texte(vec![Morceau { texte: "☑ œufs".into(), couleur: SOMBRE, fort: false, barre: true }])
        );
        assert_eq!(
            en_clair(&c, Element::Questions, auj, "10:00"),
            "À CONFIRMER · 5 | Une | Deux | Trois | + 2 AUTRES · DIS-LE-MOI À L'ORAL"
        );
        // Le souper d'un autre jour ne compte pas.
        assert_eq!(
            en_clair(&c, Element::Souper, "2026-10-27", "10:00"),
            "SOUPER CE SOIR | aucun menu prévu · demande-moi un menu de la semaine"
        );
    }

    #[test]
    fn l_encaisse_compte_les_mois_comme_la_page() {
        let budget = json!({
            "debut": "2026-09-01", "fin": "2027-02-28", "manqueMensuel": [300, 500],
            "remises": [{ "d": "2026-11-15", "m": 1250 }, { "d": "2026-10-01", "m": 99 }]
        });
        let c = centre(&[("EVE_BUDGET", budget)]);
        // 53 jours écoulés sur 180 : 1,74 mois de 30,4 jours, soit 523 $ et 872 $, arrondis à 50 $.
        let blocs = c.blocs("2026-10-24", "10:00", 0.0);
        let (_, b) = blocs.iter().find(|(e, _)| *e == Element::Encaisse).unwrap();
        assert_eq!(
            b.en_clair(),
            "ENCAISSE · SEPT. À FÉVR. · ESTIMATION | [jauge] | écart cumulé ≈ 500 $ à 850 $ | PROCHAINE REMISE · \
             15\u{a0}nov. · ≈\u{a0}1\u{202f}250\u{a0}$ · DANS\u{a0}22\u{a0}J"
        );
        let Ligne::Jauge { part, marques } = &b.lignes[0] else { panic!("la jauge d'abord") };
        assert!((part - 53.0 / 180.0).abs() < 1e-6);
        assert_eq!(marques.len(), 2);
        assert!((marques[0] - 75.0 / 180.0).abs() < 1e-6);
        // Avant la période : dans combien de jours elle commence, la jauge vide.
        let blocs = c.blocs("2026-08-22", "10:00", 0.0);
        let (_, b) = blocs.iter().find(|(e, _)| *e == Element::Encaisse).unwrap();
        assert!(b.en_clair().contains("| début dans 10 j |"), "{}", b.en_clair());
        assert_eq!(b.lignes[0], Ligne::Jauge { part: 0.0, marques: vec![75.0 / 180.0, 30.0 / 180.0] });
    }

    #[test]
    fn la_famille_vient_de_son_fichier_sinon_du_secours() {
        let c = Centre::default();
        assert!(c.famille().is_null());
        assert!(c.personnes().is_empty());
        c.famille_de_secours(json!({ "personnes": [{ "nom": "Sam" }] }));
        assert_eq!(c.famille()["personnes"][0]["nom"], "Sam");
        let js = [
            "// Une famille inventée.",
            "window.EVE_FAMILLE = {",
            "  personnes: [",
            "    { nom: 'Ana', couleur: [10, 20, 30.4], motifs: ['bureau', ' '] },",
            "    { nom: '  ' },",
            "    { nom: 'Béa', couleur: '#FF7aB8' },",
            "    { nom: 'Cy', couleur: 'rouge' },",
            "  ],",
            "  ecole: { motifs: ['les castors'] },",
            "};",
        ];
        c.valeurs.lock().unwrap().extend(affectations(&js.join("\n")));
        let noms: Vec<(String, [u8; 3], Vec<String>)> =
            c.personnes().into_iter().map(|p| (p.nom, p.couleur, p.motifs)).collect();
        assert_eq!(
            noms,
            [
                ("Ana".into(), [10, 20, 30], vec!["bureau".into()]),
                ("Béa".into(), [255, 122, 184], vec![]),
                // Une couleur illisible : celle de son rang (le troisième quartier).
                ("Cy".into(), [255, 210, 63], vec![])
            ],
            "le fichier l'emporte ; une personne sans nom ne compte pas"
        );
        assert_eq!(c.motifs_de_l_ecole(), ["les castors"]);
        // Huit personnes au plus.
        let neuf: Vec<Value> = (0..9).map(|k| json!({ "nom": format!("P{k}") })).collect();
        c.valeurs.lock().unwrap().insert("EVE_FAMILLE".into(), json!({ "personnes": neuf }));
        assert_eq!(c.personnes().len(), 8);
        assert_eq!(c.personnes()[7].couleur, COULEURS[7]);
    }

    #[test]
    fn la_tempete_de_novembre_a_avril() {
        let c = Centre::default();
        c.famille_de_secours(json!({ "ecole": { "site": "ecoles.exemple.ca" } }));
        let tempete = |c: &Centre, auj: &str| en_clair(c, Element::Tempete, auj, "10:00");
        assert_eq!(tempete(&c, "2026-11-15"), "", "sans météo, rien");
        c.tempete(Some((4.6, 69.4)));
        assert_eq!(tempete(&c, "2026-11-15"), "", "sous les deux seuils");
        c.tempete(Some((5.0, 0.0)));
        assert_eq!(
            tempete(&c, "2026-11-15"),
            "TEMPÊTE POSSIBLE | 5 cm de neige · rafales 0 km/h | VÉRIFIE LES ÉCOLES : ECOLES.EXEMPLE.CA"
        );
        c.tempete(Some((0.4, 70.0)));
        assert_eq!(
            tempete(&c, "2027-04-30"),
            "TEMPÊTE POSSIBLE | 0 cm de neige · rafales 70 km/h | VÉRIFIE LES ÉCOLES : ECOLES.EXEMPLE.CA"
        );
        assert!(c.blocs("2027-04-30", "10:00", 0.0).iter().any(|(e, b)| *e == Element::Tempete && b.alerte));
        assert!(c.alerte_famille("2027-04-30", "10:00"), "la destination de la famille sonne");
        // Hors saison, même un gros orage ne fait pas de tempête de neige.
        c.tempete(Some((30.0, 110.0)));
        for auj in ["2026-10-31", "2027-05-01", "2027-08-15"] {
            assert_eq!(tempete(&c, auj), "", "{auj}");
            assert!(!c.alerte_famille(auj, "10:00"));
        }
        assert_eq!(
            tempete(&c, "2026-12-24"),
            "TEMPÊTE POSSIBLE | 30 cm de neige · rafales 110 km/h | VÉRIFIE LES ÉCOLES : ECOLES.EXEMPLE.CA"
        );
        // Sans site d'école : la tempête seule.
        c.famille_de_secours(Value::Null);
        assert_eq!(tempete(&c, "2027-01-10"), "TEMPÊTE POSSIBLE | 30 cm de neige · rafales 110 km/h");
    }

    #[test]
    fn la_cueillette_suit_ses_jours_et_sa_plage() {
        let c = Centre::default();
        let (lundi, samedi) = ("2026-10-26", "2026-10-24");
        assert!(!c.cueillette(lundi, "16:45"), "sans famille, pas de cueillette");
        c.famille_de_secours(json!({ "cueillette": {
            "debut": "16:40", "fin": "17:00", "texte": "Va chercher Noah", "note": "RETARD : 2 $ LA MINUTE"
        } }));
        assert!(!c.cueillette(lundi, "16:39"));
        assert!(c.cueillette(lundi, "16:40"), "son début compte");
        assert!(c.cueillette(lundi, "16:59"));
        assert!(!c.cueillette(lundi, "17:00"), "sa fin, non");
        assert!(!c.cueillette(samedi, "16:45"), "en semaine d'office");
        assert!(c.alerte_famille(lundi, "16:45"));
        // Sans titre : « CUEILLETTE · FERMETURE » et l'heure de sa fin ; en alerte.
        let blocs = c.blocs(lundi, "16:45", 0.0);
        let (_, b) = blocs.iter().find(|(e, _)| *e == Element::Cueillette).unwrap();
        assert!(b.alerte);
        assert_eq!(b.en_clair(), "CUEILLETTE · FERMETURE 17 H | Va chercher Noah | RETARD : 2 $ LA MINUTE");
        // Ses jours à elle (le samedi et le dimanche), son titre à elle.
        c.famille_de_secours(
            json!({ "cueillette": { "jours": [6, 7], "debut": "09:00", "fin": "09:30", "titre": "AU CLUB" } }),
        );
        assert!(c.cueillette(samedi, "09:10"));
        assert!(!c.cueillette(lundi, "09:10"));
        assert_eq!(en_clair(&c, Element::Cueillette, samedi, "09:10"), "AU CLUB");
        // Sans début ou sans fin : jamais.
        c.famille_de_secours(json!({ "cueillette": { "fin": "17:00" } }));
        assert!(!c.cueillette(lundi, "16:59"));
    }

    #[test]
    fn le_compte_a_rebours_des_anniversaires_et_du_conge() {
        let famille = json!({
            "personnes": [
                { "nom": "Marc", "anniversaire": "03-15" },
                { "nom": "Léa", "couleur": [255, 122, 184], "anniversaire": "2020-12-01" },
                { "nom": "Noah", "anniversaire": "2023-11-02" },
                { "nom": "Bébé", "anniversaire": "2027-01-10" },
                { "nom": "Zoé", "anniversaire": "2016-02-29" },
                { "nom": "Max", "anniversaire": "2025-10-24" }
            ],
            "ecole": { "conge_de": "léa" }
        });
        let echeances = json!([
            { "d": "2026-10-20", "q": "Passé", "t": "ecole" },
            { "d": "2026-11-13", "q": "Congé", "t": "ecole" },
            { "d": "2026-11-01", "q": "Autre", "t": "famille" }
        ]);
        let b = bloc_compte(&famille, &echeances, "2026-10-24").unwrap();
        assert_eq!(
            b.en_clair(),
            "COMPTE À REBOURS | Max · 1 an AUJOURD'HUI | Noah · 3 ans dans 9 j | Léa · 6 ans dans 38 j | Zoé · 11 ans \
             dans 128 j | Prochain congé de léa · 20 j"
        );
        // Le nombre est un `<b>` de la couleur de la personne (la sienne, ou celle de son rang : Noah est la
        // troisième) ; le congé prend celle de la personne qu'il nomme.
        let rose = Color32::from_rgb(255, 122, 184);
        assert_eq!(
            b.lignes[1],
            Ligne::Texte(vec![texte("Noah · 3 ans dans "), fort("9", couleur32(COULEURS[2])), texte(" j")])
        );
        assert_eq!(b.lignes[2], Ligne::Texte(vec![texte("Léa · 6 ans dans "), fort("38", rose), texte(" j")]));
        assert_eq!(b.lignes[4], Ligne::Texte(vec![texte("Prochain congé de léa · "), fort("20", rose), texte(" j")]));
        // L'élision ; sans personne nommée, le congé d'école, en jaune ; le jour même.
        let b = bloc_compte(&json!({ "ecole": { "conge_de": "Émile" } }), &echeances, "2026-11-13").unwrap();
        assert_eq!(b.en_clair(), "COMPTE À REBOURS | Prochain congé d'Émile · AUJOURD'HUI");
        let b = bloc_compte(&Value::Null, &echeances, "2026-11-01").unwrap();
        assert_eq!(
            b.lignes[0],
            Ligne::Texte(vec![texte("Prochain congé d'école · "), fort("12", hud::JAUNE), texte(" j")])
        );
        // Rien à compter : pas d'élément.
        assert_eq!(
            bloc_compte(
                &json!({ "personnes": [{ "nom": "Marc", "anniversaire": "03-15" }] }),
                &json!([]),
                "2026-10-24"
            ),
            None
        );
        assert_eq!(bloc_compte(&Value::Null, &echeances, "2026-11-14"), None, "plus de congé à venir");
    }

    #[test]
    fn le_point_du_jour() {
        let c = centre(&[
            (
                "EVE_AGENDA",
                json!({ "rdv": [
                    { "d": "2026-10-24", "h": "16:30", "titre": "Dentiste" },
                    { "d": "2026-10-24", "h": "09:00", "titre": "Passé" },
                    { "d": "2026-10-24", "h": "", "titre": "Collecte des feuilles" },
                    { "d": "2026-10-25", "h": "10:00", "titre": "Demain" }
                ] }),
            ),
            (
                "EVE_ECHEANCES",
                json!([
                    { "d": "2026-10-25", "q": "Journée pédagogique", "t": "ecole" },
                    { "d": "2026-11-20", "q": "Loin", "t": "famille" }
                ]),
            ),
            ("EVE_DECISION", json!({ "texte": "Réserver la gardienne.", "avant": "2026-10-27" })),
            (
                "EVE_RAPPELS",
                json!([
                    { "a": "2026-10-24 17:00:00", "texte": "appeler le plombier", "minuterie": false },
                    { "a": "2026-10-25 09:00:00", "texte": "demain", "minuterie": false }
                ]),
            ),
            ("EVE_OUTLOOK", json!({ "boites": [{ "nom": "a", "non_lus": 2 }, { "nom": "b", "non_lus": 1 }] })),
        ]);
        assert_eq!(
            c.point_du_jour("2026-10-24", "10:00"),
            [
                "Aujourd'hui : Dentiste à 16 h 30 et Collecte des feuilles.",
                "À surveiller : Journée pédagogique (demain).",
                "La décision du jour : Réserver la gardienne.",
                "Rappels : à 17 h, appeler le plombier.",
                "3 courriels non lus.",
            ]
        );
        assert!(centre(&[]).point_du_jour("2026-10-24", "10:00").is_empty());
    }

    #[test]
    fn les_rappels_a_venir() {
        let r = json!([
            { "a": "2026-10-25 16:00:00", "texte": "appeler le plombier", "minuterie": false },
            { "a": "2026-10-25 16:15:30", "texte": "La minuterie de 10 minutes est finie.", "minuterie": true },
            { "a": "2026-10-26 09:00:00", "texte": "téléphoner à Marie", "minuterie": false },
            { "a": "2026-10-27 08:00:00", "texte": "le notaire", "minuterie": false }
        ]);
        assert_eq!(
            bloc_rappels(&r, "2026-10-25").unwrap().en_clair(),
            "RAPPELS · 4 À VENIR | 16 h · appeler le plombier | 16 h 15 · minuterie de 10 minutes | demain à 9 h · \
             téléphoner à Marie | + 1 AUTRE"
        );
        assert_eq!(bloc_rappels(&json!([]), "2026-10-25"), None);
    }

    #[test]
    fn la_journee_en_chiffres_vient_du_journal() {
        let journal = "\
2026-10-23 23:59:59 outil ▸ WebSearch
2026-10-24 07:58:12 outil ▸ Read : C:\\Essai\\activite\\echeances.js
2026-10-24 07:58:15 outil ▸ Glob : activite/*.js
2026-10-24 08:02:40 outil ▸ PowerShell : $o = New-Object -ComObject Outlook.Application
2026-10-24 08:10:00 outil ▸ WebSearch
2026-10-24 08:10:04 outil ▸ WebFetch
2026-10-24 08:11:30 outil ▸ Edit : D:\\Ailleurs\\notes.md
2026-10-24 09:15:00 outil ▸ Skill : courriels
2026-10-24 10:20:00 outil ▸ Bash : python scripts/meteo.py
2026-10-24 10:21:00 outil ▸ mcp__claude-in-chrome__navigate
2026-10-24 12:05:00 outil ▸ mcp__exemple__lister
2026-10-24 12:05:01 outil rejeté avant la garde (rien n'a tourné) : mcp__exemple__lister (toolu_essai)
2026-10-24 13:40:00 Eve répond (1,2 s).
2026-10-24 13:40:01 fin du tour (2.3 s, 0.0400 $ ; cumul 0.3100 $) : Voilà.
2026-10-24 13:52:10 fin du tour (9.8 s, 0.0820 $ ; cumul 0.3920 $) : C'est fait.
2026-10-25 00:00:01 outil ▸ Read : demain.md
";
        let d = decompter(journal, "2026-10-24", Path::new(r"C:\Essai"));
        assert_eq!(
            d,
            Decompte { actions: 10, web: 3, courriels: 1, fichiers: 3, autres: 3, demandes: 2, cout: 0.0400 + 0.0820 }
        );
        assert_eq!(decompter(journal, "2026-10-26", Path::new(r"C:\Essai")), Decompte::default());
        // Le panneau ne dit les chiffres que du jour que le fil a comptés.
        let c = Centre::default();
        assert_eq!(en_clair(&c, Element::Chiffres, "2026-10-24", "10:00"), "");
        assert_eq!(c.journee.lock().unwrap().voulu, "2026-10-24", "le fil comptera ce jour-là");
        {
            let mut j = c.journee.lock().unwrap();
            j.compte = "2026-10-24".into();
            j.decompte = d;
        }
        assert_eq!(
            en_clair(&c, Element::Chiffres, "2026-10-24", "10:00"),
            "JOURNÉE EN CHIFFRES | 2 demandes · ≈ 0,12 $ | 10 actions · 3 web · 1 courriel | 3 fichiers · 3 commandes et \
             outils | DEPUIS MINUIT"
        );
        assert_eq!(en_clair(&c, Element::Chiffres, "2026-10-25", "00:00"), "", "minuit passé : le fil recompte");
    }

    #[test]
    fn les_echeances_et_leurs_alertes() {
        let c = centre(&[(
            "EVE_ECHEANCES",
            json!([
                { "d": "2026-11-10", "q": "Plus tard", "t": "famille" },
                { "d": "2026-10-24", "q": "Aujourd'hui", "t": "famille" },
                { "d": "2026-10-25", "q": "Demain", "t": "auto" },
                { "d": "2026-10-31", "q": "École dans 7 jours", "t": "ecole" },
                { "d": "2026-10-23", "q": "Hier", "t": "fiscal" },
                { "d": "2026-10-30", "q": "Une sorte inconnue", "t": "autre" },
                { "d": "2027-04-01", "q": "Trop loin", "t": "fiscal" }
            ]),
        )]);
        let e = c.echeances("2026-10-24");
        let titres: Vec<(String, bool)> = e.iter().map(|b| (b.en_clair(), b.alerte)).collect();
        assert_eq!(
            titres,
            [
                ("AUJOURD'HUI | Aujourd'hui | 24 oct.".to_string(), true),
                ("DEMAIN | Demain | 25 oct.".to_string(), true),
                ("DANS 6 J | Une sorte inconnue | 30 oct.".to_string(), false),
                ("DANS 7 J | École dans 7 jours | 31 oct.".to_string(), true),
            ]
        );
        assert_eq!(e[1].couleur, Some(hud::VERT));
        assert_eq!(e[3].couleur, Some(hud::JAUNE));
        // Le tableau de bord en reçoit une carte : chaque date en mot de tête (rouge sous son seuil), le délai en
        // détail ; combien pressent dans la pilule.
        let datees: Vec<String> = c.echeances_datees("2026-10-24").into_iter().map(|(d, _)| d).collect();
        assert_eq!(datees, ["2026-10-24", "2026-10-25", "2026-10-30", "2026-10-31"]);
        let l = liste_des_echeances(&c.echeances_datees("2026-10-24"));
        assert_eq!(l.pilule, Some(("3 qui pressent".to_string(), erp::ROUGE)));
        assert_eq!(l.rangees[0].bouts[0], theme::Bout::tete("sam. 24 oct.", erp::DANGER_ENCRE));
        assert_eq!(l.rangees[2].detail, "Dans 6 jours");
        assert!(c.echeances_datees("2027-06-01").is_empty(), "plus rien dans les 150 jours");
    }

    #[test]
    fn les_delais_en_toutes_lettres() {
        assert_eq!(delai_lisible("DANS 5 J"), "Dans 5 jours");
        assert_eq!(delai_lisible("AUJOURD'HUI"), "Aujourd'hui");
        assert_eq!(delai_lisible("DEMAIN"), "Demain");
        assert_eq!(delai_lisible("AVANT LE 30 OCT. · DANS 7 J"), "Avant le 30 oct. · dans 7 jours");
        assert_eq!(delai_lisible("ÉCHÉANCE DÉPASSÉE"), "Échéance dépassée");
        assert_eq!(delai_lisible("DANS 1 J"), "Dans 1 jour");
    }

    #[test]
    fn les_rendez_vous_du_jour_et_de_demain() {
        let a = serde_json::json!({"rdv": [
            {"d": "2026-10-23", "h": "16:30", "titre": "Dentiste"},
            {"d": "2026-10-23", "h": "09:00", "titre": "Passé"},
            {"d": "2026-10-24", "h": "", "titre": "Journée pédagogique"},
            {"d": "2026-10-26", "h": "10:00", "titre": "Plus tard"}
        ]});
        let r = rendez_vous(&a, "2026-10-23", "16:00");
        let titres: Vec<(&str, Option<i64>)> = r.iter().map(|(v, d)| (champ(v, "titre"), *d)).collect();
        assert_eq!(titres, [("Dentiste", Some(30)), ("Journée pédagogique", None)]);
    }

    #[test]
    fn le_flottement_et_le_clignotement() {
        assert_eq!(flottement(0.0), 0.0);
        assert!((flottement(3.0) - 1.0).abs() < 1e-6, "au milieu de la période, tout en haut");
        assert!((flottement(1.5) - 0.5).abs() < 1e-6);
        assert!((flottement(4.5) - 0.5).abs() < 1e-6);
        assert!(flottement(6.0).abs() < 1e-6);
        assert!((va_et_vient(1.6, 3.2) - 1.0).abs() < 1e-6, "le souffle de la décision, à son plus fort");
        assert_eq!(de("Léa"), "de Léa");
        assert_eq!(de("Émile"), "d'Émile");
        assert_eq!(de("Hugo"), "d'Hugo");
        assert_eq!(prochain_anniversaire("05-17", "2026-10-24"), None, "sans l'année");
        assert_eq!(prochain_anniversaire("2020-12-01", "2026-12-02"), Some((364, 7)));
        assert_eq!(prochain_anniversaire("2026-11-01", "2026-10-24"), None, "pas encore né");
    }

    #[test]
    fn la_decision_du_jour_et_les_couleurs_des_cartes() {
        // La décision et son délai, dépassé ou non ; le tableau et les cartes la prennent au même endroit.
        let c = centre(&[("EVE_DECISION", json!({ "texte": "Réserver la gardienne", "avant": "2026-10-26" }))]);
        let suite = Some("AVANT LE 26 OCT. · DANS 2 J".to_string());
        let attendu = Decision { texte: "Réserver la gardienne".into(), suite, depassee: false };
        assert_eq!(c.decision("2026-10-24"), Some(attendu));
        assert_eq!(c.decision("2026-10-26").unwrap().suite.as_deref(), Some("AVANT CE SOIR"));
        assert!(c.decision("2026-10-27").unwrap().depassee);
        // Sans décision : l'échéance la plus proche dans les quatorze jours, sinon rien.
        let c = centre(&[(
            "EVE_ECHEANCES",
            json!([{ "d": "2026-11-02", "q": "Congé", "t": "ecole" }, { "d": "2026-12-01", "q": "Loin" }]),
        )]);
        let attendu = Decision { texte: "Congé".into(), suite: Some("DANS 9 J".into()), depassee: false };
        assert_eq!(c.decision("2026-10-24"), Some(attendu));
        assert_eq!(c.decision("2026-10-01"), None, "rien dans les quatorze jours");
        // Les couleurs du tableau, en clair : chacune a la sienne ; celle d'une personne se lit sur le blanc.
        assert_eq!(sur_clair(hud::CYAN), erp::PRIMAIRE);
        assert_eq!(sur_clair(SOMBRE), erp::TEXTE_3);
        let rose = Color32::from_rgb(255, 122, 184);
        assert_ne!(sur_clair(rose), rose, "trop pâle sur le blanc, elle s'assombrit");
        assert_eq!(teinte_de_sorte(Some(hud::JAUNE)), erp::JAUNE);
        assert_eq!(teinte_de_sorte(None), erp::GRIS);
    }

    /// Un tableau de comptabilité inventé, de la forme de son `format-donnees.md` : ses échéances (l'une longue, l'une
    /// mal datée, l'une sans texte), les remises de ses personnes (deux le même jour, une passée, deux mal écrites).
    fn tableau_invente() -> Value {
        json!({
            "version": 1,
            "misAJour": "2026-10-20",
            "echeances": [
                { "date": "2026-11-02", "quoi": "Acompte provisionnel (fédéral et Québec) ; à verser en ligne",
                  "pourQui": "Alex" },
                { "date": "2026-12-15", "quoi": "Remise des retenues", "pourQui": "Société exemple inc." },
                { "date": "le 3 mars", "quoi": "Une date mal écrite" },
                { "date": "2026-11-20" }
            ],
            "personnes": [
                { "nom": "Alex", "type": "adulte",
                  "remises": [{ "nom": "Solde d'impôt", "montant": 300, "echeance": "2027-04-30" }] },
                { "nom": "Société exemple inc.", "type": "entreprise", "remises": [
                    { "nom": "Retenues", "montant": 1480, "echeance": "2026-11-15" },
                    { "nom": "TPS et TVQ", "montant": 2150, "echeance": "2027-01-31" },
                    { "nom": "Impôt de la société", "montant": 640, "echeance": "2027-01-31" },
                    { "nom": "Passée", "montant": 900, "echeance": "2026-10-01" },
                    { "nom": "Sans montant", "echeance": "2026-12-01" },
                    { "nom": "Un montant écrit en texte", "montant": "1 000 $", "echeance": "2026-12-01" }
                ] },
                { "nom": "Sam" }
            ]
        })
    }

    /// Un fichier d'essai qui dit `texte`, daté de `secondes` plus tard : deux écritures de suite peuvent tomber sur la
    /// même date, au pas de l'horloge du disque.
    fn ecrire_date(chemin: &Path, texte: &str, secondes: u64) {
        std::fs::write(chemin, texte).unwrap();
        let f = std::fs::File::options().write(true).open(chemin).unwrap();
        f.set_modified(SystemTime::now() + Duration::from_secs(secondes)).unwrap();
    }

    #[test]
    fn le_tableau_de_la_comptabilite_se_lit_comme_les_copies() {
        let t = tableau_de(&tableau_invente());
        assert_eq!(t.maj.as_deref(), Some("2026-10-20"));
        assert_eq!(
            t.echeances,
            [
                json!({ "d": "2026-11-02", "q": "Acompte provisionnel", "t": "fiscal" }),
                json!({ "d": "2026-12-15", "q": "Remise des retenues", "t": "fiscal" })
            ]
        );
        let remises: Vec<(&str, f64)> = t.remises.iter().map(|r| (r.date.as_str(), r.montant)).collect();
        assert_eq!(
            remises,
            [
                ("2027-04-30", 300.0),
                ("2026-11-15", 1480.0),
                ("2027-01-31", 2150.0),
                ("2027-01-31", 640.0),
                ("2026-10-01", 900.0)
            ],
            "les remises de toutes les personnes"
        );
        assert_eq!(t.ignores, 4, "deux échéances et deux remises mal écrites, laissées de côté");
        // Un tableau neuf, sans rien de tout cela : rien, sans erreur.
        assert_eq!(tableau_de(&json!({ "version": 1, "misAJour": "pas une date" })), Tableau::default());
    }

    #[test]
    fn un_texte_de_la_comptabilite_se_raccourcit() {
        assert_eq!(
            court("Acompte provisionnel (fédéral et Québec) ; à verser en ligne", COURT),
            "Acompte provisionnel"
        );
        assert_eq!(court("  Remise   des retenues. ", COURT), "Remise des retenues");
        assert_eq!(court("Déclaration (voir la note", COURT), "Déclaration", "une parenthèse jamais fermée");
        assert_eq!(court("(tout entre parenthèses)", COURT), "(tout entre parenthèses)", "il n'en resterait rien");
        assert_eq!(court("Remise des retenues à la source de la société par actions", 30), "Remise des retenues à la…");
        assert_eq!(court("Abcdefghijklmnopqrstuvwxyz0123456789", 20), "Abcdefghijklmnopqrs…", "un seul mot, coupé net");
        assert!(court(&"mot ".repeat(40), COURT).chars().count() <= COURT);
    }

    #[test]
    fn la_comptabilite_nommee_remplace_les_copies() {
        let c = centre(&[
            (
                "EVE_ECHEANCES",
                json!([
                    { "d": "2026-10-28", "q": "Copie fiscale", "t": "fiscal" },
                    { "d": "2026-10-30", "q": "Pédagogique", "t": "ecole" },
                    { "d": "2026-11-05", "q": "Sans sorte" }
                ]),
            ),
            (
                "EVE_BUDGET",
                json!({ "debut": "2026-10-01", "fin": "2027-01-31", "manqueMensuel": [100, 200],
                        "remises": [{ "d": "2026-11-20", "m": 999 }] }),
            ),
        ]);
        let auj = "2026-10-26";
        let echeances = |c: &Centre| c.echeances(auj).iter().map(Bloc::en_clair).collect::<Vec<_>>();
        let encaisse = |c: &Centre| en_clair(c, Element::Encaisse, auj, "10:00");
        // Sans comptabilité, les copies, comme avant.
        assert_eq!(echeances(&c)[0], "DANS 2 J | Copie fiscale | 28 oct.");
        assert!(encaisse(&c).starts_with("ENCAISSE · OCT. À JANV. · ESTIMATION | [jauge]"), "{}", encaisse(&c));
        // Nommée, son tableau pas encore lu : ni la copie fiscale ni celle du budget — rien de vieux en attendant.
        c.suivre_la_comptabilite(r" D:\Exemple\Comptes ");
        assert_eq!(echeances(&c), ["DANS 4 J | Pédagogique | 30 oct.", "DANS 10 J | Sans sorte | 5 nov."]);
        assert_eq!(encaisse(&c), "");
        // Lu : ses échéances fiscales, à leur rang parmi les autres, et ses remises.
        c.comptabilite.lock().unwrap().tableau = Some(Ok(tableau_de(&tableau_invente())));
        assert_eq!(
            echeances(&c),
            [
                "DANS 4 J | Pédagogique | 30 oct.",
                "DANS 7 J | Acompte provisionnel | 2 nov.",
                "DANS 10 J | Sans sorte | 5 nov.",
                "DANS 50 J | Remise des retenues | 15 déc."
            ]
        );
        assert!(c.echeances(auj)[1].alerte, "fiscale, à moins de 14 jours : l'alerte, comme une copie");
        assert_eq!(
            encaisse(&c),
            "ENCAISSE · COMPTABILITÉ DU 20 OCT. | ≈\u{a0}4\u{202f}570\u{a0}$ à remettre d'ici le\u{a0}30\u{a0}avr. | \
             PROCHAINE REMISE · 15\u{a0}nov. · ≈\u{a0}1\u{202f}480\u{a0}$ · DANS\u{a0}20\u{a0}J"
        );
        // La décision du jour, sans la sienne, prend l'échéance la plus proche : ici, celle de la comptabilité.
        let d = c.decision("2026-10-31").unwrap();
        assert_eq!((d.texte.as_str(), d.suite.as_deref()), ("Acompte provisionnel", Some("DANS 2 J")));
        // Son tableau ne se lit plus : ni ses chiffres ni les copies, et l'encaisse dit pourquoi.
        c.comptabilite.lock().unwrap().tableau = Some(Err("est introuvable".into()));
        assert_eq!(echeances(&c), ["DANS 4 J | Pédagogique | 30 oct.", "DANS 10 J | Sans sorte | 5 nov."]);
        assert_eq!(
            encaisse(&c),
            "ENCAISSE · COMPTABILITÉ | tableau\\donnees.json est introuvable | L'ENCAISSE ET LES ÉCHÉANCES FISCALES \
             EN VIENNENT"
        );
        // Plus de comptabilité nommée : les copies reviennent.
        c.suivre_la_comptabilite("");
        assert_eq!(echeances(&c).len(), 3);
        assert!(encaisse(&c).contains("ESTIMATION"));
    }

    #[test]
    fn l_encaisse_de_la_comptabilite_dit_ses_remises_a_venir() {
        let remise = |date: &str, montant: f64| Remise { date: date.into(), montant };
        let t =
            Tableau { remises: vec![remise("2026-11-15", 1480.0), remise("2026-10-01", 900.0)], ..Tableau::default() };
        // Une seule à venir : la prochaine, sans total ; sans date du tableau, le titre seul.
        assert_eq!(
            bloc_remises(&t, "2026-11-15").unwrap().en_clair(),
            "ENCAISSE · COMPTABILITÉ | PROCHAINE REMISE · 15\u{a0}nov. · ≈\u{a0}1\u{202f}480\u{a0}$ · \
             DANS\u{a0}0\u{a0}J"
        );
        // Plus rien à venir : rien.
        assert_eq!(bloc_remises(&t, "2026-11-16"), None);
        assert_eq!(bloc_remises(&Tableau::default(), "2026-11-16"), None);
    }

    /// Le tableau relu quand il change : lu, il se montre aussitôt ; une version qui ne se lit pas attend d'être restée
    /// la même d'un passage à l'autre, puis plus rien d'elle ne se montre ; réparé, il se relit.
    #[test]
    fn le_tableau_se_relit_quand_il_change_et_ne_montre_rien_de_vieux() {
        let travail = std::env::temp_dir().join(format!("eve-centre-comptabilite-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&travail);
        let fichier = travail.join("Comptes").join("tableau").join("donnees.json");
        std::fs::create_dir_all(fichier.parent().unwrap()).unwrap();
        let c = Centre::default();
        let mut suivi = Suivi::default();
        let passer = |suivi: &mut Suivi| relire_la_comptabilite(&c.comptabilite, suivi, &travail);
        let montre = || c.comptabilite.lock().unwrap().tableau.clone();
        assert!(!passer(&mut suivi), "aucune comptabilité nommée : rien à lire");
        // Un chemin relatif part du dossier de travail ; un tableau lu se montre aussitôt, une fois.
        ecrire_date(&fichier, &tableau_invente().to_string(), 0);
        c.suivre_la_comptabilite("Comptes");
        assert!(passer(&mut suivi));
        assert_eq!(montre(), Some(Ok(tableau_de(&tableau_invente()))));
        assert!(!passer(&mut suivi), "rien n'a changé");
        // Une version qui ne se lit pas s'écrit peut-être encore : la fenêtre garde ce qu'elle montrait…
        ecrire_date(&fichier, "{\"echeances\": [", 5);
        assert!(!passer(&mut suivi));
        assert!(matches!(montre(), Some(Ok(_))));
        // … puis, restée la même, plus rien d'elle ne se montre, et l'encaisse dit pourquoi.
        assert!(passer(&mut suivi));
        let raison = montre().unwrap().unwrap_err();
        assert!(raison.starts_with("ne se lit pas (ligne 1, colonne "), "{raison}");
        assert!(!passer(&mut suivi));
        // Réparé, il se relit aussitôt.
        ecrire_date(&fichier, "{\"misAJour\": \"2026-10-22\", \"echeances\": []}", 10);
        assert!(passer(&mut suivi));
        assert_eq!(montre(), Some(Ok(Tableau { maj: Some("2026-10-22".into()), ..Tableau::default() })));
        // Nommée ailleurs puis ici de nouveau entre deux passages : la fenêtre l'avait oubliée, il se relit.
        c.suivre_la_comptabilite("Ailleurs");
        c.suivre_la_comptabilite("Comptes");
        assert_eq!(montre(), None);
        assert!(passer(&mut suivi));
        assert!(matches!(montre(), Some(Ok(_))));
        // Retiré : introuvable, le passage d'attente fait.
        std::fs::remove_file(&fichier).unwrap();
        assert!(!passer(&mut suivi));
        assert!(passer(&mut suivi));
        assert_eq!(montre(), Some(Err("est introuvable".into())));
        // Ce qui n'est pas un objet JSON, ce qui n'est pas en UTF-8.
        ecrire_date(&fichier, "[1, 2]", 15);
        assert!(!passer(&mut suivi));
        assert!(passer(&mut suivi));
        assert_eq!(montre(), Some(Err("n'est pas un objet JSON".into())));
        std::fs::write(&fichier, b"{\"misAJour\": \"\xe9t\xe9\"}").unwrap();
        assert_eq!(lire_le_tableau(&fichier), Err("n'est pas écrit en UTF-8".into()));
        let _ = std::fs::remove_dir_all(&travail);
    }

    /// Le fil lit aussitôt une comptabilité nommée, sans attendre son pas de deux secondes.
    #[test]
    fn le_fil_lit_aussitot_une_comptabilite_nommee() {
        let travail = std::env::temp_dir().join(format!("eve-centre-fil-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&travail);
        let fichier = travail.join("Comptes").join("tableau").join("donnees.json");
        std::fs::create_dir_all(fichier.parent().unwrap()).unwrap();
        std::fs::write(&fichier, tableau_invente().to_string()).unwrap();
        let c = Centre::avec_journal(Some(travail.clone()), None, egui::Context::default());
        // Le fil a fait son premier passage (rien dans `activite\`), et attend.
        std::thread::sleep(Duration::from_millis(100));
        let debut = Instant::now();
        c.suivre_la_comptabilite("Comptes");
        while c.comptabilite.lock().unwrap().tableau.is_none() && debut.elapsed() < Duration::from_millis(1900) {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(matches!(&c.comptabilite.lock().unwrap().tableau, Some(Ok(t)) if t.echeances.len() == 2));
        assert!(debut.elapsed() < Duration::from_millis(1500), "lue en {:?}", debut.elapsed());
        drop(c);
        let _ = std::fs::remove_dir_all(&travail);
    }

    /// Le planning de la maison, sur les fichiers du tableau de bord : les échéances (l'école à part), la décision du
    /// jour, les rappels (pas les minuteries), les anniversaires de la fenêtre à l'an prochain (pas avant le premier),
    /// l'agenda — tout du côté de la famille, dans l'ordre du calendrier. Sans l'entreprise, ni Gantt ni Kanban.
    #[test]
    fn le_planning_de_la_maison() {
        use crate::planning::{Cote, Sorte};
        let c = centre(&[
            (
                "EVE_ECHEANCES",
                json!([
                    { "d": "2026-11-13", "q": "Journée pédagogique", "t": "ecole" },
                    { "d": "2026-11-02", "q": "Pneus d'hiver", "t": "auto" },
                    { "d": "pas une date", "q": "Perdue", "t": "famille" }
                ]),
            ),
            ("EVE_DECISION", json!({ "texte": "Réserver la gardienne.", "avant": "2026-10-30" })),
            (
                "EVE_RAPPELS",
                json!([
                    { "a": "2026-10-28 17:00:00", "texte": "appeler le plombier", "minuterie": false },
                    { "a": "2026-10-28 16:10:00", "texte": "La minuterie de 10 minutes est finie.", "minuterie": true }
                ]),
            ),
            (
                "EVE_FAMILLE",
                json!({ "personnes": [
                    { "nom": "Léa", "anniversaire": "2020-12-01" },
                    { "nom": "Émile", "anniversaire": "03-15" },
                    { "nom": "Bébé", "anniversaire": "2027-01-10" }
                ] }),
            ),
            ("EVE_AGENDA", json!({ "t": 1.0, "rdv": [{ "d": "2026-10-29", "h": "9:30", "titre": "Dentiste" }] })),
        ]);
        let p = c.planning("2026-10-28");
        assert!(p.taches.is_empty() && p.dependances.is_empty() && p.tableaux.is_empty() && p.lu.is_empty());
        let vu: Vec<(&str, Option<&str>, &str, Sorte)> =
            p.evenements.iter().map(|e| (e.debut.as_str(), e.heure.as_deref(), e.titre.as_str(), e.sorte)).collect();
        assert_eq!(
            vu,
            [
                ("2026-10-28", Some("17:00"), "appeler le plombier", Sorte::Rappel),
                ("2026-10-29", Some("09:30"), "Dentiste", Sorte::RendezVous),
                ("2026-10-30", None, "Décision : Réserver la gardienne", Sorte::Echeance),
                ("2026-11-02", None, "Pneus d'hiver", Sorte::Echeance),
                ("2026-11-13", None, "Journée pédagogique", Sorte::Ecole),
                ("2026-12-01", None, "Anniversaire de Léa", Sorte::Anniversaire),
                ("2027-03-15", None, "Anniversaire d'Émile", Sorte::Anniversaire),
            ]
        );
        assert!(p.evenements.iter().all(|e| e.cote == Cote::Famille));
        let lea = &p.evenements[5];
        assert_eq!(lea.detail, "6 ans");
        assert_eq!(lea.source, "l'anniversaire de Léa (6 ans), le mardi 1er décembre");
        assert_eq!(p.evenements[0].source, "le rappel d'Eve « appeler le plombier », le mercredi 28 octobre à 17 h");
        assert_eq!(p.evenements[4].detail, "École");
        // Le planning ne se refait que quand ce que lit le centre a changé (le fil le compte) : la fenêtre peut le
        // demander à chaque image.
        c.valeurs.lock().unwrap().insert("EVE_DECISION".into(), json!({}));
        assert!(Arc::ptr_eq(&c.planning("2026-10-28"), &p), "rien de compté : le même, pas refait");
        c.generation.fetch_add(1, Ordering::Release);
        assert_eq!(c.planning("2026-10-28").evenements.len(), 6, "la décision prise, elle s'en va");
        // Les anniversaires d'une date de naissance : son âge quand l'année est connue ; le 29 février, le 1er mars.
        assert_eq!(
            anniversaires("2016-02-29", "2027-01-01", "2028-12-31"),
            [("2027-03-01".to_string(), Some(11)), ("2028-02-29".to_string(), Some(12))]
        );
        assert_eq!(anniversaires("11-02", "2026-09-01", "2027-10-29"), [("2026-11-02".to_string(), None)]);
        assert!(anniversaires("le 2 novembre", "2026-09-01", "2027-10-29").is_empty());
    }
}
