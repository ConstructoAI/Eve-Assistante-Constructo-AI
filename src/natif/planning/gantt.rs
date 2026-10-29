//! Le Gantt du tableau de bord (l'onglet « Gantt ») : un tableau du suivi de Constructo AI dans le temps, au choix comme
//! au Kanban — les chantiers et leurs opérations, les soumissions, les bons de travail et leurs opérations, les
//! opérations, les ventes, les achats, les factures —, d'après la compétence de visualisation des données. À gauche, la
//! liste — un chantier ou un bon de travail, son chevron qui le replie ou le déplie, ses opérations en retrait ; une
//! soumission, une vente, une facture, chacune sur sa rangée ; à droite, le temps — l'échelle des semaines (leurs
//! lundis, les fins de semaine pâlies) ou des mois, des filets pleins et pâles, la ligne d'aujourd'hui en marine.
//! Chaque tâche, sa barre fine, aux
//! bouts arrondis, de la couleur de son statut (planifiée en gris, en cours en bleu, terminée en vert, en attente en
//! ambre, en retard en rouge — un état, toujours dit aussi par la légende et la bulle) ; son avancement, la part faite
//! en plein et le reste en plus pâle de la même couleur, écrit au bout d'une tâche en cours ; quand elle glisse, sa
//! référence (les dates prévues de l'ERP) en trait gris sous elle, et son retard écrit au bout (« + 5 j ») ; un jalon,
//! un losange. Les dépendances : des flèches fines, en équerre, de la fin d'une tâche au début de celle qui l'attend.
//! Au survol, la rangée se lave et sa bulle dit tout ; un clic ouvre son menu : en parler, me le rappeler demain,
//! « Replanifier… ». Au-dessus : le choix du tableau ; puis son titre, ‹ Aujourd'hui ›, ses données en tableau (chaque
//! valeur s'y lit sans survol), « Tout déplier », « Semaines · Mois ».

use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, FontFamily, FontId, Layout, Pos2, Rect, Sense, Shape, Stroke,
    UiBuilder, pos2, vec2,
};

use super::{
    Action, Cle, FormeDeCle, bornes_du_mois, bulle, choix_du_tableau, couleur_de_la_personne, en_francais, initiales,
    jour_de_semaine, jours_de_la_tache, legende, lundi, menu_de_chose, navigation, nom_du_mois, parler_de_la_tache,
    premier_jour, quantieme, rappel_de_la_tache, replanifier_la_tache, titre_de_vue,
};
use crate::centre;
use serde_json::{Value, json};

use crate::natif::theme::{self, BoutonErp, erp};
use crate::natif::{ICONES, SEMI};
use crate::planning::{Dependance, Gantt, Planning, Tache};

/// L'échelle du temps : les semaines (un jour fait une quinzaine de points) ou les mois.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Echelle {
    Semaines,
    Mois,
}

/// Ce que montre le Gantt : le tableau du suivi (sa clé ; vide : le premier, les chantiers), son échelle, de combien de
/// pas il a avancé ou reculé depuis aujourd'hui (quatre semaines, ou deux mois, par pas), les groupes qu'on a dépliés
/// ou repliés (leur id) — les autres suivent la règle d'office ([`ouvert`]) —, et ses données en tableau à la place des
/// barres.
#[derive(Clone, Debug, PartialEq)]
struct Etat {
    tableau: String,
    echelle: Echelle,
    decalage: i64,
    ouverts: Vec<(String, bool)>,
    en_tableau: bool,
}

impl Default for Etat {
    fn default() -> Self {
        Etat { tableau: String::new(), echelle: Echelle::Semaines, decalage: 0, ouverts: Vec::new(), en_tableau: false }
    }
}

const ETAT: &str = "etat-du-gantt";

fn etat(ctx: &egui::Context) -> Etat {
    ctx.data(|d| d.get_temp::<Etat>(egui::Id::unique(ETAT))).unwrap_or_default()
}

fn garder(ctx: &egui::Context, e: Etat) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::unique(ETAT), e));
}

/// Le Gantt d'une scène de l'essai du thème : ce tableau (sa clé ; vide : le premier), ces groupes dépliés (les autres
/// suivent la règle d'office), aux mois ou aux semaines.
pub(super) fn preparer(ctx: &egui::Context, deplies: &[&str], mois: bool, en_tableau: bool, tableau: &str) {
    let ouverts = deplies.iter().map(|id| (id.to_string(), true)).collect();
    let echelle = if mois { Echelle::Mois } else { Echelle::Semaines };
    garder(ctx, Etat { tableau: tableau.into(), echelle, ouverts, en_tableau, ..Etat::default() });
}

/// Ce qu'un Gantt appelle ses rangées : l'en-tête de sa liste, la colonne de sa vue en tableau, ce qu'est la rangée
/// au-dessus d'une rangée en retrait, ce qu'on replanifie, et ce que dit son Gantt vide (son titre, son texte).
struct Mots {
    liste: &'static str,
    colonne: &'static str,
    parent: &'static str,
    chose: &'static str,
    vide: (&'static str, &'static str),
}

/// Les mots du Gantt d'un tableau du suivi (sa clé : celles de [`crate::planning::TABLEAUX`]).
fn mots(cle: &str) -> Mots {
    match cle {
        "projets" => Mots {
            liste: "CHANTIER · OPÉRATION",
            colonne: "Chantier · opération",
            parent: "chantier",
            chose: "la tâche",
            vide: (
                "Aucun chantier planifié",
                "Aucun chantier n'a de dates dans Constructo AI : ses projets et leurs opérations se placeront ici dès \
                 qu'ils en auront.",
            ),
        },
        "devis" => Mots {
            liste: "SOUMISSION",
            colonne: "Soumission",
            parent: "soumission",
            chose: "la soumission",
            vide: (
                "Aucune soumission à placer",
                "Aucune soumission en cours n'a de dates dans Constructo AI : leurs travaux prévus se placeront ici.",
            ),
        },
        "bons_travail" => Mots {
            liste: "BON DE TRAVAIL · OPÉRATION",
            colonne: "Bon de travail · opération",
            parent: "bon de travail",
            chose: "la tâche",
            vide: ("Aucun bon de travail planifié", "Aucun bon de travail n'a de dates dans Constructo AI."),
        },
        "operations" => Mots {
            liste: "OPÉRATION",
            colonne: "Opération",
            parent: "bon de travail",
            chose: "l'opération",
            vide: (
                "Aucune opération planifiée",
                "Aucune opération du suivi de production n'a de dates : elles se lisent dans Constructo AI quand ce \
                 Gantt s'ouvre.",
            ),
        },
        "ventes" => Mots {
            liste: "VENTE",
            colonne: "Vente",
            parent: "vente",
            chose: "la vente",
            vide: ("Aucune vente à placer", "Aucune vente ouverte du CRM n'a de dates dans Constructo AI."),
        },
        "achats" => Mots {
            liste: "BON DE COMMANDE",
            colonne: "Bon de commande",
            parent: "bon de commande",
            chose: "le bon de commande",
            vide: ("Aucun achat en route", "Aucun bon de commande en route n'a de dates dans Constructo AI."),
        },
        "factures" => Mots {
            liste: "FACTURE",
            colonne: "Facture",
            parent: "facture",
            chose: "la facture",
            vide: ("Aucune facture à suivre", "Aucune facture n'attend son solde dans Constructo AI."),
        },
        _ => Mots {
            liste: "TÂCHE",
            colonne: "Tâche",
            parent: "groupe",
            chose: "la tâche",
            vide: ("Rien de planifié", "Ce tableau n'a encore rien de daté dans Constructo AI."),
        },
    }
}

/// Les tableaux dont le Gantt montre les opérations du suivi de production : elles se demandent quand il se montre.
fn montre_les_operations(cle: &str) -> bool {
    matches!(cle, "projets" | "bons_travail" | "operations")
}

/// Au-delà de tant de rangées, les chantiers se montrent repliés d'office (la personne déplie ce qu'elle veut voir).
const REPLIES_AU_DELA: usize = 24;

/// Un chantier est-il déplié : ce qu'en a dit la personne, sinon la règle d'office — déplié tant que le Gantt entier
/// tient dans [`REPLIES_AU_DELA`] rangées.
fn ouvert(e: &Etat, id: &str, total: usize) -> bool {
    e.ouverts.iter().find(|(x, _)| x == id).map_or(total <= REPLIES_AU_DELA, |(_, o)| *o)
}

/// Où en est une tâche : son statut dans l'ERP, ou, sans lui, ses dates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Etape {
    Planifiee,
    EnCours,
    Terminee,
    EnAttente,
    EnRetard,
}

impl Etape {
    /// La couleur d'un état, celle des pastilles de l'ERP (lisible sur le blanc, sauf le gris de ce qui n'a pas
    /// commencé : la légende et la bulle le disent aussi).
    fn couleur(self) -> Color32 {
        match self {
            Etape::Planifiee => PLANIFIEE,
            Etape::EnCours => erp::BLEU.vif[1],
            Etape::Terminee => erp::VERT.vif[1],
            Etape::EnAttente => erp::AMBRE.vif[1],
            Etape::EnRetard => erp::ROUGE.vif[1],
        }
    }

    fn nom(self) -> &'static str {
        match self {
            Etape::Planifiee => "Planifiée",
            Etape::EnCours => "En cours",
            Etape::Terminee => "Terminée",
            Etape::EnAttente => "En attente",
            Etape::EnRetard => "En retard",
        }
    }
}

/// L'état d'une tâche (de `debut` à `fin`) à un jour : son statut de l'ERP (terminé, en retard, en attente, planifié,
/// en cours — sans égard aux accents ni à la casse) ; une tâche finie à 100 % est terminée ; passée sa fin sans l'être,
/// elle est en retard ; un statut inconnu se lit par les dates (commencée : en cours ; sinon planifiée).
fn etape(t: &Tache, debut: i64, fin: i64, aujourdhui: i64) -> Etape {
    let s = eve::parole::normaliser(&t.statut);
    let a = |mots: &[&str]| mots.iter().any(|m| s.contains(m));
    if t.avancement.is_some_and(|x| x >= 0.999) || a(&["termin", "complet", "ferme", "fait", "acheve", "livre"]) {
        return Etape::Terminee;
    }
    if a(&["retard"]) || fin < aujourdhui {
        return Etape::EnRetard;
    }
    if a(&["attente", "pause", "suspend", "bloqu"]) {
        return Etape::EnAttente;
    }
    if a(&["planif", "a venir", "a faire", "nouveau", "brouillon"]) {
        return Etape::Planifiee;
    }
    if a(&["cours", "actif", "demarr", "commence"]) || debut <= aujourdhui { Etape::EnCours } else { Etape::Planifiee }
}

/// Une rangée du Gantt : sa tâche (son rang dans son Gantt), ses jours, son niveau (0 une chose du tableau, 1 l'une
/// des siennes, en retrait), combien elle en a, le titre de celle au-dessus d'elle, et si elle est un groupe (un
/// chantier, un bon de travail et ses opérations : sa rangée plus haute, sa barre de synthèse, son chevron).
#[derive(Clone, Debug, PartialEq)]
struct Rangee {
    k: usize,
    debut: i64,
    fin: i64,
    niveau: u8,
    enfants: usize,
    parent: Option<String>,
    groupe: bool,
}

/// Les rangées du Gantt : chaque chose du tableau (une tâche sans parent, ou dont le parent n'est pas là), puis les
/// siennes, dans l'ordre du planning (celui de l'ERP) — celles d'un groupe replié ne se montrent pas. Les chantiers sont
/// des groupes, et les choses d'un tableau où l'une en a d'autres sous elle (les bons de travail et leurs opérations) ;
/// ailleurs, chacune est une tâche sur sa rangée. Une tâche dont les dates ne se lisent pas s'en va ; une fin avant le
/// début compte pour un jour. Rend aussi combien de rangées aurait le Gantt déplié.
fn rangees(g: &Gantt, e: &Etat) -> (Vec<Rangee>, usize) {
    let jours = |t: &Tache| {
        let debut = centre::numero_du_jour(&t.debut)?;
        let fin = centre::numero_du_jour(&t.fin).filter(|f| *f >= debut).unwrap_or(debut);
        Some((debut, fin))
    };
    let est_la = |id: &str| g.taches.iter().any(|t| t.id == id);
    let hauts: Vec<usize> = (0..g.taches.len())
        .filter(|k| g.taches[*k].parent.as_deref().is_none_or(|parent| !est_la(parent)))
        .filter(|k| jours(&g.taches[*k]).is_some())
        .collect();
    let en_groupes = g.cle == "projets" || g.taches.iter().any(|t| t.parent.as_deref().is_some_and(est_la));
    let total = g.taches.iter().filter(|t| jours(t).is_some()).count();
    let mut sortie = Vec::new();
    for c in hauts {
        let t = &g.taches[c];
        let Some((debut, fin)) = jours(t) else { continue };
        let enfants: Vec<(usize, i64, i64)> = (0..g.taches.len())
            .filter(|k| *k != c && g.taches[*k].parent.as_deref() == Some(t.id.as_str()))
            .filter_map(|k| jours(&g.taches[k]).map(|(d, f)| (k, d, f)))
            .collect();
        let rangee = Rangee { k: c, debut, fin, niveau: 0, enfants: enfants.len(), parent: None, groupe: en_groupes };
        sortie.push(rangee);
        if ouvert(e, &t.id, total) {
            for (k, debut, fin) in enfants {
                let parent = Some(t.titre.clone());
                sortie.push(Rangee { k, debut, fin, niveau: 1, enfants: 0, parent, groupe: false });
            }
        }
    }
    (sortie, total)
}

/// Les jours que montre le Gantt : de `debut`, `jours` jours.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Plage {
    debut: i64,
    jours: i64,
}

impl Plage {
    fn fin(&self) -> i64 {
        self.debut + self.jours
    }
}

/// La plage du Gantt dans une largeur `l` de temps : aux semaines, autant de semaines qu'il en tient à 17 points par
/// jour (de quatre à vingt-six), depuis le lundi d'il y a deux semaines ; aux mois, autant de mois qu'il en tient à 140
/// points (de trois à dix-huit), depuis le mois dernier. Chaque pas de `decalage` : quatre semaines, ou deux mois.
fn plage(echelle: Echelle, decalage: i64, aujourdhui: i64, l: f32) -> Plage {
    match echelle {
        Echelle::Semaines => {
            let semaines = ((l / (7.0 * 17.0)).floor() as i64).clamp(4, 26);
            Plage { debut: lundi(aujourdhui) - 14 + 28 * decalage, jours: 7 * semaines }
        }
        Echelle::Mois => {
            let mois = ((l / 140.0).floor() as i64).clamp(3, 18);
            let (a, m) = super::annee_et_mois(aujourdhui);
            let debut = premier_jour(a, m - 1 + 2 * decalage);
            let (a, m) = super::annee_et_mois(debut);
            Plage { debut, jours: premier_jour(a, m + mois) - debut }
        }
    }
}

/// Le chemin d'une dépendance, en équerre, de `de` (le bout de la tâche qu'on attend : `de_la_fin`, sa fin) à `vers`
/// (le bout de celle qui attend : `vers_le_debut`, son début) ; quand il n'y a pas la place d'aller tout droit, il
/// passe entre les deux rangées (`entre`).
fn chemin(de: Pos2, de_la_fin: bool, vers: Pos2, vers_le_debut: bool, entre: f32) -> Vec<Pos2> {
    let sortie = de.x + if de_la_fin { 8.0 } else { -8.0 };
    let entree = vers.x + if vers_le_debut { -8.0 } else { 8.0 };
    let droit = if vers_le_debut { entree >= sortie } else { entree <= sortie };
    if droit {
        vec![de, pos2(sortie, de.y), pos2(sortie, vers.y), vers]
    } else {
        vec![de, pos2(sortie, de.y), pos2(sortie, entre), pos2(entree, entre), pos2(entree, vers.y), vers]
    }
}

/// Les bouts d'une dépendance, d'après sa sorte (celle de l'ERP, `FS` d'office) : part-elle de la fin de la tâche
/// qu'on attend (sinon de son début), va-t-elle au début de celle qui attend (sinon à sa fin) ?
fn bouts(d: &Dependance) -> (bool, bool) {
    match d.sorte.trim().to_uppercase().as_str() {
        "SS" => (false, true),
        "FF" => (true, false),
        "SF" => (false, false),
        _ => (true, true),
    }
}

/// Où une dépendance part et où elle arrive, en x, d'après ses bouts ([`bouts`]) : de la fin ou du début de la tâche
/// qu'on attend (`de` : le x de son début, celui de sa fin), au début ou à la fin de celle qui attend (`vers`).
fn x_des_bouts(d: &Dependance, de: (f32, f32), vers: (f32, f32)) -> (f32, f32) {
    let (de_la_fin, vers_le_debut) = bouts(d);
    (if de_la_fin { de.1 } else { de.0 }, if vers_le_debut { vers.0 } else { vers.1 })
}

/// La hauteur d'une rangée : un chantier, une opération.
const CHANTIER: f32 = 36.0;
const OPERATION: f32 = 32.0;
/// La hauteur des barres : celle d'un chantier (sa barre de synthèse), d'une opération.
const BARRE_DE_CHANTIER: f32 = 12.0;
const BARRE: f32 = 16.0;
/// La couleur des dépendances, de la ligne d'aujourd'hui.
const FLECHE: Color32 = Color32::from_rgb(0x8A, 0x96, 0xA8);
const AUJOURDHUI: Color32 = erp::MARINE_HAUT;
/// Une tâche qui n'a pas commencé : un gris pâle, de contexte, sous les couleurs de ce qui avance ; la référence (les
/// dates prévues), un trait gris plus foncé et fin, sous la barre.
const PLANIFIEE: Color32 = Color32::from_rgb(0xC3, 0xCD, 0xDA);
const REFERENCE: Color32 = Color32::from_rgb(0x7C, 0x8A, 0x9E);

/// Le Gantt : le choix du tableau du suivi, sa barre d'outils, puis sa carte — la légende, l'en-tête du temps, les
/// rangées. Rend ce qu'on a choisi dans le menu d'une tâche ; montré en tableau, ce tableau (le format des cartes
/// « tableau »), que la fenêtre dessine sous la barre d'outils comme ceux des cartes ; et si le Gantt montré a les
/// opérations du suivi (elles se demandent alors au chef).
pub(super) fn vue(ui: &mut egui::Ui, p: &Planning, aujourdhui: i64) -> (Option<Action>, Option<String>, bool) {
    if p.gantts.is_empty() {
        let texte = "Constructo AI n'a encore aucun tableau de suivi à montrer dans le temps : ses chantiers, ses \
                     soumissions et ses bons de travail s'y placeront.";
        super::etat_vide(ui, super::ONGLETS[2].1, erp::BLEU, "Aucun tableau de suivi", texte, None);
        return (None, None, false);
    }
    let avant = etat(ui.ctx());
    let mut e = avant.clone();
    let titres: Vec<String> = p.gantts.iter().map(|g| en_francais(&g.titre)).collect();
    let mut k = p.gantts.iter().position(|g| g.cle == e.tableau).unwrap_or(0);
    if let Some(j) = choix_du_tableau(ui, "tableau-du-gantt", &titres, k, super::ONGLETS[2].1).filter(|j| *j != k) {
        k = j;
        e.tableau = p.gantts[j].cle.clone();
        e.decalage = 0;
    }
    ui.add_space(10.0);
    let g = &p.gantts[k];
    let m = mots(&g.cle);
    let operations = montre_les_operations(&g.cle);
    let (rangees, total) = rangees(g, &e);
    let largeur = ui.available_width();
    // La barre d'outils.
    let (barre, _) = ui.allocate_exact_size(vec2(largeur, 40.0), Sense::hover());
    let mut gauche = ui.new_child(UiBuilder::new().max_rect(barre).layout(Layout::left_to_right(Align::Center)));
    titre_de_vue(&mut gauche, &titres[k]);
    gauche.add_space(18.0);
    let aides = match e.echelle {
        Echelle::Semaines => ("Quatre semaines plus tôt", "Quatre semaines plus tard"),
        Echelle::Mois => ("Deux mois plus tôt", "Deux mois plus tard"),
    };
    if let Some(pas) = navigation(&mut gauche, "gantt", aides, e.decalage == 0) {
        e.decalage = if pas == 0 { 0 } else { e.decalage + pas };
    }
    let mut droite = ui.new_child(UiBuilder::new().max_rect(barre).layout(Layout::right_to_left(Align::Center)));
    let echelles = [
        theme::Segment { icone: None, mot: "Semaines", aide: "Les semaines, jour par jour" },
        theme::Segment { icone: None, mot: "Mois", aide: "Les mois : les chantiers entiers" },
    ];
    let rang = if e.echelle == Echelle::Semaines { 0 } else { 1 };
    if let Some(k) = theme::segments_erp(&mut droite, "echelle-du-gantt", &echelles, rang, 34.0)
        && k != rang
    {
        e.echelle = if k == 0 { Echelle::Semaines } else { Echelle::Mois };
        e.decalage = 0;
    }
    let groupes: Vec<&str> =
        rangees.iter().filter(|r| r.niveau == 0 && r.enfants > 0).map(|r| g.taches[r.k].id.as_str()).collect();
    if !groupes.is_empty() {
        droite.add_space(10.0);
        let tous_ouverts = groupes.iter().all(|id| ouvert(&e, id, total));
        let (mot, icone) = if tous_ouverts { ("Tout replier", '\u{E70E}') } else { ("Tout déplier", '\u{E70D}') };
        let id = Some(egui::Id::unique("tout-deplier-le-gantt"));
        if theme::bouton_erp_nomme(&mut droite, id, Some(icone), mot, BoutonErp::Fantome).clicked() {
            e.ouverts = groupes.iter().map(|id| (id.to_string(), !tous_ouverts)).collect();
        }
    }
    // Les barres, ou leurs données en tableau.
    if !rangees.is_empty() {
        droite.add_space(10.0);
        let (icone, aide) = if e.en_tableau {
            (super::ONGLETS[2].1, "Voir le Gantt")
        } else {
            (crate::cartes::pictos::TABLEAU, "Voir les données : chaque tâche, ses jours et son statut, en tableau")
        };
        if super::bouton_rond(&mut droite, icone, aide, "gantt-en-tableau") {
            e.en_tableau = !e.en_tableau;
        }
    }
    ui.add_space(12.0);
    if rangees.is_empty() {
        super::etat_vide(ui, super::ONGLETS[2].1, erp::BLEU, m.vide.0, m.vide.1, None);
        if e != avant {
            garder(ui.ctx(), e);
        }
        return (None, None, operations);
    }
    if e.en_tableau {
        let tableau = en_tableau(g, &rangees, aujourdhui, m.colonne);
        if e != avant {
            garder(ui.ctx(), e);
        }
        return (None, Some(tableau), operations);
    }
    let mut action = None;
    theme::carte(ui, None, 0.0, |ui| {
        action = gantt(ui, g, &rangees, &mut e, total, aujourdhui, &m);
    });
    if e != avant {
        garder(ui.ctx(), e);
    }
    (action, None, operations)
}

/// Le Gantt en tableau, au format des cartes « tableau » (`colonnes`, `lignes` ; `colonne` : le titre de la première) :
/// les rangées montrées — une chose du tableau, puis les siennes dépliées, en retrait —, leurs jours, leur statut, leur
/// avancement, leur référence quand elles glissent, leurs responsables ; chaque valeur s'y lit sans survol.
fn en_tableau(g: &Gantt, rangees: &[Rangee], aujourdhui: i64, colonne: &str) -> String {
    let lignes: Vec<Value> = rangees
        .iter()
        .map(|r| {
            let t = &g.taches[r.k];
            let titre = if r.niveau > 0 { format!("— {}", t.titre.trim()) } else { t.titre.trim().to_string() };
            let statut = statut_dit(t, etape(t, r.debut, r.fin, aujourdhui));
            let reference = reference_dite(t, glissement(t, r.fin))
                .map_or(String::new(), |(quand, sens)| format!("{} · {sens}", theme::casse_de_phrase(&quand)));
            json!([
                titre,
                date_du_tableau(&t.debut, aujourdhui),
                date_du_tableau(&t.fin, aujourdhui),
                statut,
                avancement_dit(t).unwrap_or_default(),
                reference,
                t.responsables.join(", "),
            ])
        })
        .collect();
    let colonnes = [colonne, "Début", "Fin", "Statut", "Fait", "Prévu", "Responsables"];
    json!({ "colonnes": colonnes, "lignes": lignes }).to_string()
}

/// Une date du tableau : « 28 mai », et son année quand ce n'est pas celle d'aujourd'hui (« 28 mai 2027 ») — le Gantt
/// court d'une année à l'autre.
fn date_du_tableau(date: &str, aujourdhui: i64) -> String {
    let courte = centre::date_courte(date);
    match date.get(..4).and_then(|a| a.parse::<i64>().ok()) {
        Some(a) if a != super::annee_et_mois(aujourdhui).0 => format!("{courte} {a}"),
        _ => courte,
    }
}

/// Le statut d'une tâche, à l'écran : celui de l'ERP, en français ; sans lui, celui que dit son état.
fn statut_dit(t: &Tache, etape: Etape) -> String {
    if t.statut.trim().is_empty() { etape.nom().to_string() } else { en_francais(&t.statut) }
}

/// L'avancement d'une tâche : « 55 % ».
fn avancement_dit(t: &Tache) -> Option<String> {
    t.avancement.map(|a| format!("{} %", (a.clamp(0.0, 1.0) * 100.0).round() as i64))
}

/// La référence d'une tâche qui glisse (`glisse` : de combien de jours) : quand elle était prévue (« du 26 sept. au
/// 21 oct. »), et de combien elle glisse (« glisse de 7 jours »).
fn reference_dite(t: &Tache, glisse: Option<i64>) -> Option<(String, String)> {
    let (g, (d, f)) = (glisse?, t.prevu.as_ref()?);
    let quand = if d == f {
        format!("le {}", centre::date_courte(d))
    } else {
        format!("du {} au {}", centre::date_courte(d), centre::date_courte(f))
    };
    let sens = match g {
        1 => "glisse d'un jour".to_string(),
        g if g > 0 => format!("glisse de {g} jours"),
        -1 => "un jour d'avance".to_string(),
        g => format!("{} jours d'avance", -g),
    };
    Some((quand, sens))
}

/// La carte du Gantt : sa légende, l'en-tête du temps, ses rangées, ses dépendances, la ligne d'aujourd'hui. Rend ce
/// qu'on a choisi dans le menu d'une tâche ; un chevron cliqué replie ou déplie son groupe (`e`).
fn gantt(
    ui: &mut egui::Ui,
    g: &Gantt,
    rangees: &[Rangee],
    e: &mut Etat,
    total: usize,
    aujourdhui: i64,
    m: &Mots,
) -> Option<Action> {
    let l = ui.available_width();
    let etapes: Vec<Etape> = rangees.iter().map(|r| etape(&g.taches[r.k], r.debut, r.fin, aujourdhui)).collect();
    let glissements: Vec<Option<i64>> = rangees.iter().map(|r| glissement(&g.taches[r.k], r.fin)).collect();
    // La légende : les états montrés, dans leur ordre ; puis la référence, les dépendances, aujourd'hui.
    let mut cles: Vec<Cle> = Vec::new();
    for x in [Etape::Planifiee, Etape::EnCours, Etape::EnAttente, Etape::EnRetard, Etape::Terminee] {
        if etapes.contains(&x) {
            cles.push(Cle { couleur: x.couleur(), forme: FormeDeCle::Pastille, icone: None, nom: x.nom().into() });
        }
    }
    if glissements.iter().any(Option::is_some) {
        let nom = "Prévu (la référence)".to_string();
        cles.push(Cle { couleur: REFERENCE, forme: FormeDeCle::Barre, icone: None, nom });
    }
    if !g.dependances.is_empty() {
        cles.push(Cle { couleur: FLECHE, forme: FormeDeCle::Trait, icone: None, nom: "Dépendance".into() });
    }
    cles.push(Cle { couleur: AUJOURDHUI, forme: FormeDeCle::Verticale, icone: None, nom: "Aujourd'hui".into() });
    legende(ui, &cles);
    ui.add_space(10.0);
    // La liste à gauche, le temps à droite.
    let gauche = (l * 0.3).clamp(190.0, 330.0);
    let temps = (l - gauche).max(120.0);
    let plage = plage(e.echelle, e.decalage, aujourdhui, temps);
    let par_jour = temps / plage.jours as f32;
    let (tete, _) = ui.allocate_exact_size(vec2(l, 44.0), Sense::hover());
    let x0 = tete.left() + gauche;
    let x_de = |n: i64| x0 + (n - plage.debut) as f32 * par_jour;
    let hauteurs: Vec<f32> = rangees.iter().map(|r| if r.groupe { CHANTIER } else { OPERATION }).collect();
    let (corps, _) = ui.allocate_exact_size(vec2(l, hauteurs.iter().sum()), Sense::hover());
    let pinceau = ui.painter().clone();
    let temps_rect = Rect::from_min_max(pos2(x0, tete.top()), pos2(tete.right(), corps.bottom()));
    let clip = pinceau.with_clip_rect(temps_rect);
    // La pastille d'aujourd'hui, dans la rangée des lundis : l'échelle ne s'écrit pas sous elle.
    let aujourdhui_se_voit = (plage.debut..plage.fin()).contains(&aujourdhui);
    let x_d_aujourdhui = x_de(aujourdhui) + par_jour / 2.0;
    let pastille = aujourdhui_se_voit.then(|| {
        let police = FontId::new(11.0, FontFamily::Name(SEMI.into()));
        let g = pinceau.layout_no_wrap("Aujourd'hui".to_string(), police, Color32::WHITE);
        let r = Rect::from_center_size(pos2(x_d_aujourdhui, tete.top() + 31.0), g.size() + vec2(14.0, 6.0));
        let r = r.translate(vec2((temps_rect.left() - r.left()).max(0.0), 0.0));
        (r.translate(vec2((temps_rect.right() - r.right()).min(0.0), 0.0)), g)
    });
    // Les fins de semaine pâlies, la grille, l'en-tête.
    let masque = pastille.as_ref().map(|(r, _)| r.expand(3.0));
    entete_du_temps(&pinceau, &clip, (tete, corps), x0, plage, par_jour, e.echelle, masque, m.liste);
    // Les rangées : leur place.
    let mut y = corps.top();
    let places: Vec<Rect> = hauteurs
        .iter()
        .map(|h| {
            let r = Rect::from_min_size(pos2(corps.left(), y), vec2(l, *h));
            y += h;
            r
        })
        .collect();
    // Les filets entre les chantiers.
    for (r, place) in rangees.iter().zip(&places).skip(1) {
        if r.niveau == 0 {
            pinceau.hline(corps.x_range(), place.top(), Stroke::new(1.0, erp::FILET_DE_CARTE));
        }
    }
    let mut action = None;
    let mut basculer = None;
    // Les rangées : leur survol, leur menu, puis la liste.
    for (i, (r, place)) in rangees.iter().zip(&places).enumerate() {
        let t = &g.taches[r.k];
        let cle = format!("tache-du-gantt-{}", t.id);
        let rep = ui.interact(*place, egui::Id::unique(cle.as_str()), Sense::click());
        let menu_ouvert = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&rep));
        // Le survol lave la rangée d'un bleu très pâle : la grille et les fins de semaine restent visibles dessous.
        if rep.hovered() || menu_ouvert {
            pinceau.rect_filled(place.shrink2(vec2(0.0, 1.0)), 6.0, erp::PRIMAIRE.gamma_multiply(0.07));
        }
        liste(ui, &pinceau, t, r, *place, gauche, e, total, &mut basculer);
        let parent = r.parent.as_deref().map(|p| (m.parent, p));
        if rep.hovered() && !menu_ouvert {
            bulle_de_la_tache(ui, t, r, (etapes[i], glissements[i]), parent, &cle);
        }
        let rep = rep.on_hover_cursor(egui::CursorIcon::PointingHand);
        let propres = [('\u{E787}', "Replanifier…".to_string())];
        action = action.or(match menu_de_chose(&rep, &cle, &propres, None) {
            Some(0) => Some(parler_de_la_tache(t, parent)),
            Some(1) => Some(rappel_de_la_tache(t, parent)),
            Some(_) => Some(replanifier_la_tache(t, parent, m.chose)),
            None => None,
        });
    }
    // Les dépendances, sous les barres.
    for d in &g.dependances {
        let trouver = |id: &str| rangees.iter().position(|r| g.taches[r.k].id == id);
        let (Some(a), Some(b)) = (trouver(&d.de), trouver(&d.vers)) else { continue };
        let (de_la_fin, vers_le_debut) = bouts(d);
        let jours = |i: usize| (x_de(rangees[i].debut), x_de(rangees[i].fin + 1));
        let (depart, arrivee) = x_des_bouts(d, jours(a), jours(b));
        let (de, vers) = (pos2(depart, places[a].center().y), pos2(arrivee, places[b].center().y));
        let entre = if places[b].center().y > places[a].center().y { places[b].top() } else { places[b].bottom() };
        let points = chemin(de, de_la_fin, vers, vers_le_debut, entre);
        clip.add(Shape::line(points, Stroke::new(1.3, FLECHE)));
        let pointe = if vers_le_debut { -1.0 } else { 1.0 };
        let fleche = vec![vers, vers + vec2(6.0 * pointe, -4.0), vers + vec2(6.0 * pointe, 4.0)];
        clip.add(Shape::convex_polygon(fleche, FLECHE, Stroke::NONE));
    }
    // Les barres ; ce qui s'écrit au bout d'une barre d'où part une flèche se pose après elle.
    for (i, (r, place)) in rangees.iter().zip(&places).enumerate() {
        let id = &g.taches[r.k].id;
        let fleche = g.dependances.iter().any(|d| d.de == *id && bouts(d).0);
        let t = &g.taches[r.k];
        barre(&pinceau, &clip, t, r, *place, etapes[i], glissements[i], x_de, plage, temps_rect, fleche);
    }
    // Aujourd'hui : sa ligne, et sa pastille dans l'en-tête.
    if let Some((pilule, g)) = pastille {
        let haut = pilule.bottom();
        clip.vline(x_d_aujourdhui, haut..=corps.bottom(), Stroke::new(2.0, AUJOURDHUI.gamma_multiply(0.85)));
        pinceau.rect_filled(pilule, 255.0, AUJOURDHUI);
        pinceau.galley(pilule.center() - g.size() / 2.0, g, Color32::WHITE);
    }
    if let Some(id) = basculer {
        let maintenant = ouvert(e, &id, total);
        e.ouverts.retain(|(x, _)| *x != id);
        e.ouverts.push((id, !maintenant));
    }
    action
}

/// De combien de jours une tâche glisse par rapport à sa référence (la fin prévue) ; `None` sans référence, ou sans
/// glissement.
fn glissement(t: &Tache, fin: i64) -> Option<i64> {
    let (_, prevue) = t.prevu.as_ref()?;
    let prevue = centre::numero_du_jour(prevue)?;
    (fin != prevue).then_some(fin - prevue)
}

/// Les fins de semaine pâlies et la grille du temps (aux semaines, un filet à chaque lundi ; aux mois, au premier de
/// chaque mois), puis l'en-tête : les mois en toutes lettres (« octobre 2026 »), en haut, et aux semaines le jour de
/// chaque lundi, dessous — sauf sous la pastille d'aujourd'hui (`masque`) ; au-dessus de la liste, ce qu'elle montre
/// (`liste` : « CHANTIER · OPÉRATION »).
#[allow(clippy::too_many_arguments)]
fn entete_du_temps(
    p: &egui::Painter,
    clip: &egui::Painter,
    (tete, corps): (Rect, Rect),
    x0: f32,
    plage: Plage,
    par_jour: f32,
    echelle: Echelle,
    masque: Option<Rect>,
    liste: &str,
) {
    let x_de = |n: i64| x0 + (n - plage.debut) as f32 * par_jour;
    let haut_de_la_grille = tete.top() + if echelle == Echelle::Semaines { 22.0 } else { 0.0 };
    if echelle == Echelle::Semaines {
        for n in plage.debut..plage.fin() {
            if jour_de_semaine(n) >= 5 {
                let r = Rect::from_min_max(pos2(x_de(n), tete.bottom()), pos2(x_de(n + 1), corps.bottom()));
                clip.rect_filled(r, 0.0, Color32::from_rgb(0xF5, 0xF7, 0xFA));
            }
        }
    }
    let police = FontId::new(12.0, FontFamily::Name(SEMI.into()));
    let petite = FontId::proportional(11.5);
    // Les mois : leur nom, un filet à leur début.
    let mut debut_du_mois = bornes_du_mois(plage.debut).0;
    while debut_du_mois < plage.fin() {
        let suivant = bornes_du_mois(debut_du_mois).1;
        let (de, a) = (x_de(debut_du_mois.max(plage.debut)), x_de(suivant.min(plage.fin())));
        if debut_du_mois >= plage.debut {
            let haut = if echelle == Echelle::Mois { haut_de_la_grille } else { tete.top() };
            let bas = if echelle == Echelle::Mois { corps.bottom() } else { tete.top() + 20.0 };
            clip.vline(de, haut..=bas, Stroke::new(1.0, erp::FILET_DE_CARTE));
        }
        let annee = &centre::date_du_numero(debut_du_mois)[..4];
        let long = format!("{} {annee}", nom_du_mois(debut_du_mois));
        let court = centre::date_courte(&centre::date_du_numero(debut_du_mois)).replace("1er ", "");
        let place = a - de - 12.0;
        let g = clip.layout_no_wrap(long, police.clone(), erp::TITRE);
        let g = if g.size().x <= place { g } else { clip.layout_no_wrap(court, police.clone(), erp::TITRE) };
        if g.size().x <= place + 6.0 {
            let y = if echelle == Echelle::Mois { tete.top() + 12.0 } else { tete.top() + 10.0 };
            clip.galley(pos2(de + 6.0, y - g.size().y / 2.0), g, erp::TITRE);
        }
        debut_du_mois = suivant;
    }
    if echelle == Echelle::Semaines {
        let mut n = lundi(plage.debut);
        while n < plage.fin() {
            if n >= plage.debut {
                clip.vline(x_de(n), haut_de_la_grille..=corps.bottom(), Stroke::new(1.0, erp::FILET_DE_CARTE));
                let g = clip.layout_no_wrap(quantieme(n).to_string(), petite.clone(), erp::TEXTE_2);
                let coin = pos2(x_de(n) + 5.0, tete.top() + 31.0 - g.size().y / 2.0);
                if masque.is_none_or(|m| !m.intersects(Rect::from_min_size(coin, g.size()))) {
                    clip.galley(coin, g, erp::TEXTE_2);
                }
            }
            n += 7;
        }
    }
    p.hline(tete.x_range(), tete.bottom() - 0.5, Stroke::new(1.0, erp::FILET_DE_CARTE));
    p.vline(x0, tete.top()..=corps.bottom(), Stroke::new(1.0, erp::FILET_DE_CARTE));
    let titre = FontId::new(11.0, FontFamily::Name(SEMI.into()));
    p.text(pos2(tete.left(), tete.bottom() - 13.0), Align2::LEFT_CENTER, liste, titre, erp::TEXTE_3);
}

/// La liste d'une rangée, à gauche : un groupe (un chantier, un bon de travail), son chevron (qui le replie ou le
/// déplie : `basculer`) et son titre en demi-gras, son nombre d'opérations ; une tâche — en retrait sous son groupe,
/// sinon contre le bord —, son titre, et les pastilles des initiales de ses responsables contre le bord.
#[allow(clippy::too_many_arguments)]
fn liste(
    ui: &mut egui::Ui,
    p: &egui::Painter,
    t: &Tache,
    r: &Rangee,
    place: Rect,
    gauche: f32,
    e: &Etat,
    total: usize,
    basculer: &mut Option<String>,
) {
    let y = place.center().y;
    let mut x = place.left() + 4.0;
    let fin = place.left() + gauche - 10.0;
    if r.groupe {
        if r.enfants > 0 {
            let chevron = Rect::from_center_size(pos2(x + 9.0, y), vec2(22.0, 22.0));
            let rep = ui.interact(chevron, egui::Id::unique(("chevron-du-gantt", t.id.as_str())), Sense::click());
            if rep.hovered() {
                p.rect_filled(chevron, 6.0, Color32::from_white_alpha(200));
            }
            let deplie = ouvert(e, &t.id, total);
            let icone = if deplie { '\u{E70D}' } else { '\u{E76C}' };
            let police = FontId::new(11.0, FontFamily::Name(ICONES.into()));
            p.text(chevron.center(), Align2::CENTER_CENTER, icone, police, erp::TEXTE_2);
            let aide = if deplie { "Replier" } else { "Voir ses opérations" };
            if rep.on_hover_text(aide).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                *basculer = Some(t.id.clone());
            }
        }
        x += 24.0;
        let compte = if r.enfants > 0 { format!("{}", r.enfants) } else { String::new() };
        let g_compte = p.layout_no_wrap(compte, FontId::proportional(11.5), erp::TEXTE_3);
        let place_du_titre = (fin - x - g_compte.size().x - 8.0).max(30.0);
        let mut job = egui::text::LayoutJob::simple_singleline(
            t.titre.trim().to_string(),
            FontId::new(13.5, FontFamily::Name(SEMI.into())),
            erp::TITRE,
        );
        job.wrap = egui::text::TextWrapping::truncate_at_width(place_du_titre);
        let g = p.layout_job(job);
        let l_titre = g.size().x;
        p.galley(pos2(x, y - g.size().y / 2.0), g, erp::TITRE);
        if g_compte.size().x > 0.0 {
            let c = Rect::from_min_size(pos2(x + l_titre + 8.0, y - 9.0), vec2(g_compte.size().x + 12.0, 18.0));
            p.rect_filled(c, 9.0, erp::GRIS.pastel.gamma_multiply(0.25));
            p.galley(c.center() - g_compte.size() / 2.0, g_compte, erp::TEXTE_2);
        }
        return;
    }
    x += if r.niveau > 0 { 30.0 } else { 8.0 };
    // Les responsables : leurs initiales, contre le bord de la liste (deux au plus, puis « +1 »).
    let mut droite = fin;
    let responsables: Vec<&String> = t.responsables.iter().filter(|n| !n.trim().is_empty()).collect();
    if gauche >= 240.0 {
        for nom in responsables.iter().take(2).rev() {
            let c = pos2(droite - 10.0, y);
            p.circle_filled(c, 10.0, couleur_de_la_personne(nom));
            let police = FontId::new(9.0, FontFamily::Name(SEMI.into()));
            p.text(c, Align2::CENTER_CENTER, initiales(nom), police, Color32::WHITE);
            droite -= 17.0;
        }
        if responsables.len() > 2 {
            p.text(
                pos2(droite - 4.0, y),
                Align2::RIGHT_CENTER,
                format!("+{}", responsables.len() - 2),
                FontId::proportional(11.0),
                erp::TEXTE_2,
            );
        }
        if !responsables.is_empty() {
            droite -= 8.0;
        }
    }
    let mut job =
        egui::text::LayoutJob::simple_singleline(t.titre.trim().to_string(), FontId::proportional(13.0), erp::TEXTE);
    job.wrap = egui::text::TextWrapping::truncate_at_width((droite - x).max(30.0));
    let g = p.layout_job(job);
    p.galley(pos2(x, y - g.size().y / 2.0), g, erp::TEXTE);
}

/// La barre d'une tâche, dans le temps (`clip` : la part du temps) : sa référence sous elle quand elle glisse ; la
/// barre de la couleur de son état, la part faite en plein et le reste en pâle ; un jalon (un seul jour), un losange ;
/// au bout, son retard (« + 5 j ») ou son avancement (en cours), après la flèche d'une dépendance qui en part
/// (`fleche`). Une tâche hors de la plage : une flèche contre le bord, et sa date.
#[allow(clippy::too_many_arguments)]
fn barre(
    p: &egui::Painter,
    clip: &egui::Painter,
    t: &Tache,
    r: &Rangee,
    place: Rect,
    etape: Etape,
    glisse: Option<i64>,
    x_de: impl Fn(i64) -> f32,
    plage: Plage,
    temps: Rect,
    fleche: bool,
) {
    let y = place.center().y;
    let couleur = etape.couleur();
    let police = FontId::new(11.5, FontFamily::Name(SEMI.into()));
    if r.fin < plage.debut || r.debut >= plage.fin() {
        let avant = r.fin < plage.debut;
        let date = centre::date_courte(&centre::date_du_numero(if avant { r.fin } else { r.debut }));
        let (texte, x, aligne) = if avant {
            (format!("‹ fini le {date}"), temps.left() + 8.0, Align2::LEFT_CENTER)
        } else {
            (format!("le {date} ›"), temps.right() - 8.0, Align2::RIGHT_CENTER)
        };
        let texte = if avant && etape != Etape::Terminee { format!("‹ jusqu'au {date}") } else { texte };
        p.text(pos2(x, y), aligne, texte, FontId::proportional(11.5), erp::TEXTE_3);
        return;
    }
    let (x1, x2) = (x_de(r.debut), x_de(r.fin + 1));
    // La référence : les dates prévues, en trait gris sous la barre.
    if glisse.is_some()
        && let Some((d, f)) = &t.prevu
        && let (Some(d), Some(f)) = (centre::numero_du_jour(d), centre::numero_du_jour(f))
    {
        let h = if r.groupe { BARRE_DE_CHANTIER } else { BARRE };
        let reference =
            Rect::from_min_max(pos2(x_de(d), y + h / 2.0 + 2.0), pos2(x_de(f.max(d) + 1), y + h / 2.0 + 5.0));
        clip.rect_filled(reference, 1.5, REFERENCE);
    }
    let jalon = r.debut == r.fin && !r.groupe;
    let bout_droit = if jalon {
        let c = pos2((x1 + x2) / 2.0, y);
        let d = 8.0;
        let losange = vec![c + vec2(0.0, -d), c + vec2(d, 0.0), c + vec2(0.0, d), c + vec2(-d, 0.0)];
        clip.add(Shape::convex_polygon(losange, couleur, Stroke::new(2.0, Color32::WHITE)));
        c.x + d
    } else {
        let h = if r.groupe { BARRE_DE_CHANTIER } else { BARRE };
        let rect = Rect::from_min_max(pos2(x1 + 1.0, y - h / 2.0), pos2((x2 - 1.0).max(x1 + 3.0), y + h / 2.0));
        let rayon = (h / 2.0).min(4.0).min(rect.width() / 2.0) as u8;
        let rayon = if r.groupe { (h / 2.0) as u8 } else { rayon };
        let fait = t.avancement.map(|a| a.clamp(0.0, 1.0));
        match fait {
            Some(a) if a < 0.999 && etape != Etape::Terminee => {
                clip.rect_filled(rect, rayon, couleur.gamma_multiply(0.26));
                if a > 0.0 {
                    let plein = Rect::from_min_max(rect.min, pos2(rect.left() + rect.width() * a, rect.bottom()));
                    let coins = CornerRadius {
                        nw: rayon,
                        sw: rayon,
                        ne: if a > 0.97 { rayon } else { 0 },
                        se: if a > 0.97 { rayon } else { 0 },
                    };
                    clip.rect_filled(plein, coins, couleur);
                }
            }
            _ => {
                clip.rect_filled(rect, rayon, couleur);
            }
        }
        rect.right()
    };
    // Au bout : le retard sur la référence, sinon l'avancement d'une tâche en cours.
    let ecrit = match (glisse, etape, t.avancement) {
        (Some(g), _, _) if g > 0 => Some((format!("+ {g} j"), erp::AVERTISSEMENT_ENCRE)),
        (Some(g), _, _) if g < 0 => Some((format!("{g} j"), erp::ECART_BON)),
        (_, Etape::EnCours | Etape::EnRetard, Some(a)) if !r.groupe || a > 0.0 => {
            Some((format!("{}\u{a0}%", (a * 100.0).round() as i64), erp::TITRE))
        }
        _ => None,
    };
    if let Some((texte, encre)) = ecrit {
        let g = p.layout_no_wrap(texte, police, encre);
        let x = bout_droit + if fleche { 16.0 } else { 6.0 };
        if x + g.size().x <= temps.right() - 2.0 {
            p.galley(pos2(x, y - g.size().y / 2.0), g, encre);
        }
    }
}

/// La bulle d'une tâche : son titre ; ce qui la situe ; sa rangée au-dessus (`parent` : ce qu'elle est, son titre) ; ses
/// jours et leur durée ; son état, son avancement ; sa référence et son glissement ; ses responsables.
fn bulle_de_la_tache(
    ui: &egui::Ui,
    t: &Tache,
    r: &Rangee,
    (etape, glisse): (Etape, Option<i64>),
    parent: Option<(&str, &str)>,
    cle: &str,
) {
    let Some(pt) = ui.ctx().pointer_hover_pos() else { return };
    let duree = r.fin - r.debut + 1;
    let duree = match duree {
        1 => "1 jour".to_string(),
        d if d < 14 => format!("{d} jours"),
        d => format!("{} semaines", (d as f64 / 7.0).round() as i64),
    };
    let mut lignes: Vec<(Option<Color32>, String)> = Vec::new();
    if !t.detail.trim().is_empty() {
        lignes.push((None, t.detail.trim().to_string()));
    }
    if let Some((nom, titre)) = parent {
        lignes.push((None, format!("{} : {titre}", theme::casse_de_phrase(nom))));
    }
    lignes.push((None, format!("{} · {duree}", theme::casse_de_phrase(&jours_de_la_tache(t)))));
    let avancement = avancement_dit(t).map_or(String::new(), |a| format!(" · {a} fait"));
    lignes.push((Some(etape.couleur()), format!("{}{avancement}", statut_dit(t, etape))));
    if let Some((quand, sens)) = reference_dite(t, glisse) {
        lignes.push((Some(REFERENCE), format!("Prévu {quand} · {sens}")));
    }
    if !t.responsables.is_empty() {
        lignes.push((None, t.responsables.join(", ")));
    }
    bulle(ui, pt, &t.titre, &lignes, cle);
}

#[cfg(test)]
mod essais {
    use super::*;

    fn n(d: &str) -> i64 {
        centre::numero_du_jour(d).unwrap()
    }

    fn tache(id: &str, parent: Option<&str>, debut: &str, fin: &str, statut: &str) -> Tache {
        Tache {
            id: id.into(),
            parent: parent.map(Into::into),
            titre: format!("Tâche {id}"),
            detail: String::new(),
            debut: debut.into(),
            fin: fin.into(),
            avancement: None,
            statut: statut.into(),
            prevu: None,
            responsables: Vec::new(),
            source: String::new(),
        }
    }

    #[test]
    fn l_etat_d_une_tache() {
        let aujourdhui = n("2026-10-30");
        let t = |statut: &str, debut: &str, fin: &str| {
            let x = tache("a", None, debut, fin, statut);
            etape(&x, n(debut), n(fin), aujourdhui)
        };
        assert_eq!(t("Terminé", "2026-10-01", "2026-10-20"), Etape::Terminee);
        assert_eq!(t("En cours", "2026-10-01", "2026-10-20"), Etape::EnRetard, "sa fin passée sans être finie");
        assert_eq!(t("En cours", "2026-10-26", "2026-11-06"), Etape::EnCours);
        assert_eq!(t("EN_ATTENTE", "2026-10-26", "2026-11-06"), Etape::EnAttente);
        assert_eq!(t("Planifié", "2026-11-02", "2026-11-06"), Etape::Planifiee);
        assert_eq!(t("", "2026-11-02", "2026-11-06"), Etape::Planifiee, "sans statut, par ses dates");
        assert_eq!(t("", "2026-10-26", "2026-11-06"), Etape::EnCours);
        let mut finie = tache("a", None, "2026-10-01", "2026-11-20", "En cours");
        finie.avancement = Some(1.0);
        assert_eq!(etape(&finie, n("2026-10-01"), n("2026-11-20"), aujourdhui), Etape::Terminee);
    }

    /// Un Gantt d'un tableau, sans dépendances.
    fn gantt_de(cle: &str, taches: Vec<Tache>) -> Gantt {
        Gantt { cle: cle.into(), titre: cle.into(), taches, dependances: Vec::new() }
    }

    #[test]
    fn les_rangees_suivent_les_chantiers_replies() {
        let g = gantt_de(
            "projets",
            vec![
                tache("ch-1", None, "2026-09-01", "2026-12-18", "En cours"),
                tache("op-2", Some("ch-1"), "2026-11-02", "2026-11-27", "Planifié"),
                tache("op-1", Some("ch-1"), "2026-09-01", "2026-10-30", "En cours"),
                tache("ch-2", None, "2026-10-05", "2026-11-20", "En cours"),
                tache("op-3", Some("absent"), "2026-10-05", "2026-10-09", "Terminé"),
                tache("op-4", Some("ch-2"), "bientôt", "2026-10-09", "Terminé"),
            ],
        );
        let (r, total) = rangees(&g, &Etat::default());
        let ids: Vec<&str> = r.iter().map(|x| g.taches[x.k].id.as_str()).collect();
        assert_eq!(
            ids,
            ["ch-1", "op-2", "op-1", "ch-2", "op-3"],
            "l'ordre du planning ; un parent absent : un chantier"
        );
        assert_eq!(total, 5, "une date qui ne se lit pas s'en va");
        assert_eq!((r[0].enfants, r[1].parent.as_deref()), (2, Some("Tâche ch-1")));
        assert!(r[0].groupe && !r[1].groupe && r[4].groupe, "les chantiers sont des groupes, même sans opérations");
        let replie = Etat { ouverts: vec![("ch-1".into(), false)], ..Etat::default() };
        let (r, _) = rangees(&g, &replie);
        assert_eq!(r.len(), 3, "replié, un chantier cache ses opérations");
        // Au-delà de vingt-quatre rangées, repliés d'office.
        let beaucoup: Vec<Tache> =
            (0..30).map(|k| tache(&format!("op-{k}"), Some("ch-1"), "2026-10-01", "2026-10-02", "")).collect();
        let grand = gantt_de("projets", [vec![tache("ch-1", None, "2026-09-01", "2026-12-18", "")], beaucoup].concat());
        assert_eq!(rangees(&grand, &Etat::default()).0.len(), 1);
        // Un tableau sans rien sous ses choses (les factures) : chacune une tâche sur sa rangée, pas un groupe.
        let factures = gantt_de(
            "factures",
            vec![
                tache("facture-1", None, "2026-10-01", "2026-10-31", "Envoyée"),
                tache("facture-2", None, "2026-10-12", "2026-10-12", "Envoyée"),
            ],
        );
        let (r, _) = rangees(&factures, &Etat::default());
        assert_eq!(r.iter().map(|x| (x.niveau, x.groupe)).collect::<Vec<_>>(), [(0, false), (0, false)]);
        assert_eq!(mots("factures").liste, "FACTURE");
        assert!(montre_les_operations("bons_travail") && !montre_les_operations("factures"));
    }

    /// Le Gantt en tableau : ses colonnes, un chantier puis ses opérations en retrait, le statut de l'ERP en français,
    /// l'avancement, la référence d'une tâche qui glisse, les responsables.
    #[test]
    fn le_gantt_se_lit_en_tableau() {
        let mut acier = tache("op-1", Some("ch-1"), "2026-09-28", "2026-10-30", "En cours");
        acier.avancement = Some(0.85);
        acier.prevu = Some(("2026-09-28".into(), "2026-10-23".into()));
        acier.responsables = vec!["Alex".into()];
        let g = gantt_de(
            "projets",
            vec![tache("ch-1", None, "2026-09-01", "2026-12-18", "En cours"), acier, {
                tache("op-2", Some("ch-1"), "2026-11-02", "2027-01-15", "Planifie")
            }],
        );
        let (r, _) = rangees(&g, &Etat::default());
        let t: Value = serde_json::from_str(&en_tableau(&g, &r, n("2026-10-30"), mots("projets").colonne)).unwrap();
        assert_eq!(t["colonnes"][0], "Chantier · opération");
        assert_eq!(t["colonnes"].as_array().unwrap().len(), 7);
        let l = &t["lignes"];
        assert_eq!(l.as_array().unwrap().len(), 3);
        assert_eq!(l[0][0], "Tâche ch-1");
        assert_eq!(
            l[1],
            json!([
                "— Tâche op-1",
                "28 sept.",
                "30 oct.",
                "En cours",
                "85 %",
                "Du 28 sept. au 23 oct. · glisse de 7 jours",
                "Alex"
            ])
        );
        assert_eq!((l[2][3].as_str(), l[2][4].as_str()), (Some("Planifié"), Some("")), "en français, sans avancement");
        assert_eq!((l[2][1].as_str(), l[2][2].as_str()), (Some("2 nov."), Some("15 janv. 2027")), "l'année d'après");
    }

    #[test]
    fn la_plage_du_temps() {
        let aujourdhui = n("2026-10-30");
        let s = plage(Echelle::Semaines, 0, aujourdhui, 1000.0);
        assert_eq!((s.debut, s.jours), (n("2026-10-12"), 56), "huit semaines, depuis deux semaines avant ce lundi");
        let plus_tard = plage(Echelle::Semaines, 1, aujourdhui, 1000.0);
        assert_eq!(plus_tard.debut, n("2026-11-09"));
        let m = plage(Echelle::Mois, 0, aujourdhui, 1000.0);
        assert_eq!(m.debut, n("2026-09-01"), "depuis le mois dernier");
        assert_eq!(m.fin(), n("2027-04-01"), "sept mois");
        assert_eq!(plage(Echelle::Mois, -1, aujourdhui, 400.0).debut, n("2026-07-01"));
    }

    #[test]
    fn une_tache_qui_glisse_et_ses_dependances() {
        let mut t = tache("a", None, "2026-09-28", "2026-10-30", "En cours");
        t.prevu = Some(("2026-09-28".into(), "2026-10-23".into()));
        assert_eq!(glissement(&t, n("2026-10-30")), Some(7));
        t.prevu = Some(("2026-09-28".into(), "2026-10-30".into()));
        assert_eq!(glissement(&t, n("2026-10-30")), None, "à l'heure : rien");
        // Fin à début, de la place : tout droit ; pas de place, entre les deux rangées.
        let droit = chemin(pos2(100.0, 10.0), true, pos2(140.0, 50.0), true, 40.0);
        assert_eq!(droit, [pos2(100.0, 10.0), pos2(108.0, 10.0), pos2(108.0, 50.0), pos2(140.0, 50.0)]);
        let detour = chemin(pos2(100.0, 10.0), true, pos2(90.0, 50.0), true, 40.0);
        assert_eq!(detour.len(), 6);
        assert_eq!(detour[3], pos2(82.0, 40.0));
        // En tableau : la référence d'une tâche qui glisse, en mots.
        assert_eq!(
            reference_dite(
                &{
                    let mut t = t.clone();
                    t.prevu = Some(("2026-09-28".into(), "2026-10-23".into()));
                    t
                },
                Some(7)
            ),
            Some(("du 28 sept. au 23 oct.".to_string(), "glisse de 7 jours".to_string()))
        );
        // Les bouts d'une dépendance, d'après sa sorte ; une sorte inconnue : de la fin au début.
        let d = |sorte: &str| Dependance { de: "a".into(), vers: "b".into(), sorte: sorte.into(), decalage: 0 };
        assert_eq!(bouts(&d("FS")), (true, true));
        assert_eq!(bouts(&d(" ss ")), (false, true));
        assert_eq!(bouts(&d("FF")), (true, false));
        assert_eq!(bouts(&d("SF")), (false, false));
        assert_eq!(bouts(&d("")), (true, true));
        // Où la flèche part et où elle arrive : la tâche qu'on attend va de 10 à 20, celle qui attend de 30 à 40. Fin à
        // début : de la fin de l'une au début de l'autre (pas à sa fin).
        let (a, b) = ((10.0, 20.0), (30.0, 40.0));
        assert_eq!(x_des_bouts(&d("FS"), a, b), (20.0, 30.0));
        assert_eq!(x_des_bouts(&d("SS"), a, b), (10.0, 30.0));
        assert_eq!(x_des_bouts(&d("FF"), a, b), (20.0, 40.0));
        assert_eq!(x_des_bouts(&d("SF"), a, b), (10.0, 40.0));
    }
}
