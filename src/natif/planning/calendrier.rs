//! Le calendrier du tableau de bord (l'onglet « Calendrier ») : un mois, d'office, ou une semaine, comme les
//! calendriers d'aujourd'hui ; en tête, le choix « Tout » ou d'un tableau du suivi de l'ERP (les chantiers, les
//! soumissions, les factures…), comme au Gantt et au Kanban. Le mois : une grille de sept colonnes, du lundi au
//! dimanche ; aujourd'hui allumé (sa case lavée de l'accent et cerclée, son numéro dans une pastille, son jour de la
//! semaine à l'accent dans l'en-tête) ; les jours d'un autre mois pâlis ; chaque événement en
//! pastille de la teinte de sa sorte — son icône, son heure, son titre —, ceux de plusieurs jours en bandes d'une case
//! à l'autre ; au-delà de ce qu'une case tient, « + 2 autres », qui ouvre sa semaine. Étroit (la conversation à côté),
//! le mois montre des points de couleur, et dessous l'agenda du jour qu'on touche. La semaine : sept colonnes, les
//! bandes en tête, puis chaque jour ses événements en cartes ; étroite, un agenda, jour après jour. Au-dessus : le
//! titre, ‹ Aujourd'hui ›, le choix « Mois · Semaine » ; dessous, la légende des sortes montrées. Un clic sur un
//! événement ouvre son menu ; au survol, sa bulle dit tout.

use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, FontFamily, FontId, Layout, Rect, Sense, Stroke, UiBuilder, pos2, vec2,
};

use super::{
    Action, Cle, FormeDeCle, allure_de, bornes_du_mois, bulle, choix_du_tableau, encre_de, jour_de_semaine,
    jour_en_trois_lettres, legende, lundi, menu_de_chose, navigation, nom_du_mois, parler_de_l_evenement, premier_jour,
    quand_de_l_evenement, quantieme, rappel_de_l_evenement, se_montre, titre_de_vue,
};
use crate::centre;
use crate::natif::tableau::Vue;
use crate::natif::theme::{self, erp};
use crate::natif::{GRAS, ICONES, SEMI};
use crate::planning::{Evenement, Planning, TABLEAUX};

/// Le mois ou la semaine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    Mois,
    Semaine,
}

/// Ce que montre le calendrier : son mode, le jour qui le situe (un jour du mois ou de la semaine montrés ; `None` :
/// aujourd'hui), le jour touché dans un mois étroit (son agenda dessous ; `None` : aujourd'hui, ou le premier du mois),
/// et le tableau du suivi choisi (sa clé ; `None` : tout).
#[derive(Clone, Debug, PartialEq)]
struct Etat {
    mode: Mode,
    repere: Option<i64>,
    choisi: Option<i64>,
    tableau: Option<String>,
}

impl Default for Etat {
    fn default() -> Self {
        Etat { mode: Mode::Mois, repere: None, choisi: None, tableau: None }
    }
}

/// Où la fenêtre garde ce que montre le calendrier.
const ETAT: &str = "etat-du-calendrier";

fn etat(ctx: &egui::Context) -> Etat {
    ctx.data(|d| d.get_temp::<Etat>(egui::Id::unique(ETAT))).unwrap_or_default()
}

fn garder(ctx: &egui::Context, e: Etat) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::unique(ETAT), e));
}

/// Le calendrier d'une scène de l'essai du thème : ce mode, autour d'aujourd'hui, ce tableau du suivi (sa clé ; vide :
/// tout).
pub(super) fn preparer(ctx: &egui::Context, mode: Mode, tableau: &str) {
    let tableau = (!tableau.is_empty()).then(|| tableau.to_string());
    garder(ctx, Etat { mode, tableau, ..Etat::default() });
}

/// Un événement placé dans le temps : son rang dans le planning, son premier et son dernier jour (compris). Ce qu'on
/// en ouvre se tient à son `id`, qu'une relecture ne change pas, pas à son rang.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Place {
    k: usize,
    debut: i64,
    fin: i64,
}

impl Place {
    /// De plusieurs jours : une bande.
    fn bande(&self) -> bool {
        self.fin > self.debut
    }
}

/// Les événements de la vue (son côté) et du tableau du suivi choisi (`tableau` ; `None` : tout), placés : ceux dont la
/// date ne se lit pas s'en vont ; une fin avant le début ne compte pas.
fn places(p: &Planning, vue: Vue, tableau: Option<&str>) -> Vec<Place> {
    p.evenements
        .iter()
        .enumerate()
        .filter(|(_, e)| se_montre(e.cote, vue))
        .filter(|(_, e)| tableau.is_none_or(|t| e.tableau.as_deref() == Some(t)))
        .filter_map(|(k, e)| {
            let debut = centre::numero_du_jour(&e.debut)?;
            let fin = e.fin.as_deref().and_then(centre::numero_du_jour).filter(|f| *f >= debut).unwrap_or(debut);
            Some(Place { k, debut, fin })
        })
        .collect()
}

/// L'ordre des choses d'une journée : celles de toute la journée d'abord, puis par heure, puis par titre.
fn ordre_du_jour(p: &Planning, a: &Place) -> (bool, String, String) {
    let e = &p.evenements[a.k];
    let heure = e.heure.clone().filter(|h| !h.trim().is_empty());
    (heure.is_some(), heure.unwrap_or_default(), e.titre.clone())
}

/// L'ordre des bandes d'une semaine (de `lundi` à son dimanche), celui où elles prennent leurs couloirs : d'abord
/// celles qui s'y montrent le plus tôt ; à égalité, celle qui a commencé avant (deux bandes venues de la semaine
/// d'avant y gardent l'ordre qu'elles avaient), puis la plus longue.
fn ranger_les_bandes(bandes: &mut [Place], lundi: i64) {
    bandes.sort_by_key(|b| (b.debut.max(lundi), b.debut, -(b.fin - b.debut), b.k));
}

/// Les couloirs de bandes dans une semaine (de `lundi` à son dimanche) : chacune le premier où elle ne chevauche
/// aucune autre, dans l'ordre donné ([`ranger_les_bandes`]).
fn couloirs(bandes: &[Place], lundi: i64) -> Vec<usize> {
    let mut fins: Vec<i64> = Vec::new();
    bandes
        .iter()
        .map(|b| {
            let (debut, fin) = (b.debut.max(lundi), b.fin.min(lundi + 6));
            match fins.iter().position(|f| *f < debut) {
                Some(c) => {
                    fins[c] = fin;
                    c
                }
                None => {
                    fins.push(fin);
                    fins.len() - 1
                }
            }
        })
        .collect()
}

/// La bulle d'un événement : son titre ; quand ; son détail ; son côté et sa sorte, la pastille de sa teinte.
fn bulle_de_l_evenement(ui: &egui::Ui, e: &Evenement, aujourdhui: i64, cle: &str) {
    let Some(pt) = ui.ctx().pointer_hover_pos() else { return };
    let (teinte, _, nom) = allure_de(e, aujourdhui);
    let cote = match e.cote {
        crate::planning::Cote::Entreprise => "Entreprise",
        crate::planning::Cote::Famille => "Famille",
    };
    let lignes =
        [(None, quand_de_l_evenement(e)), (None, e.detail.clone()), (Some(teinte.vif[1]), format!("{cote} · {nom}"))];
    bulle(ui, pt, &e.titre, &lignes, cle);
}

/// Le menu d'un événement, qu'un clic sur lui (`rep`) ouvre : en parler, me le rappeler demain.
fn menu_de_l_evenement(rep: &egui::Response, e: &Evenement, cle: &str) -> Option<Action> {
    match menu_de_chose(rep, cle, &[], None)? {
        0 => Some(parler_de_l_evenement(e)),
        _ => Some(rappel_de_l_evenement(e)),
    }
}

/// Ce qu'un événement dit dans une pastille : son heure (« 7 h »), puis son titre.
fn libelle(e: &Evenement) -> (String, String) {
    let heure = e.heure.as_deref().filter(|h| !h.trim().is_empty()).map(centre::heure_parlee).unwrap_or_default();
    (heure, e.titre.trim().to_string())
}

/// Une pastille d'événement dans `r` : un fond lavé de la teinte de sa sorte (plus franc pour une bande, et au survol),
/// les coins carrés où une bande continue (`coupe` : au début, à la fin), son icône de la teinte, son heure en
/// demi-gras et son titre à l'encre du texte, coupés d'une ellipse ; un jour passé, plus pâle. Sa bulle au survol, son
/// menu au clic. `cle` la nomme (un événement de plusieurs jours a une pastille par semaine).
fn pastille(
    ui: &mut egui::Ui,
    r: Rect,
    e: &Evenement,
    aujourdhui: i64,
    passe: bool,
    coupe: (bool, bool),
    cle: &str,
) -> Option<Action> {
    let rep = ui.interact(r, egui::Id::unique(("evenement-du-calendrier", cle)), Sense::click());
    let ouvert = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&rep));
    let (teinte, icone, _) = allure_de(e, aujourdhui);
    let bande = coupe.0 || coupe.1 || e.fin.as_deref().is_some_and(|f| f != e.debut);
    let force = match (rep.hovered() || ouvert, bande, passe) {
        (true, _, _) => 0.5,
        (false, true, false) => 0.42,
        (false, true, true) => 0.26,
        (false, false, false) => 0.22,
        (false, false, true) => 0.13,
    };
    let rayon = |coupe: bool| if coupe { 0 } else { 6 };
    let coins = CornerRadius { nw: rayon(coupe.0), sw: rayon(coupe.0), ne: rayon(coupe.1), se: rayon(coupe.1) };
    let p = ui.painter().clone();
    p.rect_filled(r, coins, teinte.pastel.gamma_multiply(force));
    // Une bande commence où son liseré de la teinte se voit ; elle continue d'une semaine à l'autre sans lui.
    if bande && !coupe.0 {
        let lisere = Rect::from_min_size(r.min, vec2(3.0, r.height()));
        let encre = teinte.vif[1].gamma_multiply(if passe { 0.55 } else { 1.0 });
        p.rect_filled(lisere, CornerRadius { nw: 6, sw: 6, ne: 0, se: 0 }, encre);
    }
    let encre = if passe { erp::TEXTE_2 } else { erp::TITRE };
    let mut x = r.left() + 7.0;
    let police = FontId::new(11.0, FontFamily::Name(ICONES.into()));
    let accent = if passe { encre_de(teinte).gamma_multiply(0.7) } else { encre_de(teinte) };
    p.text(pos2(x + 5.0, r.center().y), Align2::CENTER_CENTER, icone, police, accent);
    x += 15.0;
    let (heure, titre) = libelle(e);
    let place = (r.right() - 5.0 - x).max(10.0);
    let mut job = egui::text::LayoutJob::default();
    if !heure.is_empty() && !bande {
        let semi = FontId::new(11.5, FontFamily::Name(SEMI.into()));
        job.append(&format!("{heure} "), 0.0, egui::text::TextFormat::simple(semi, encre));
    }
    job.append(&titre, 0.0, egui::text::TextFormat::simple(FontId::proportional(11.5), encre));
    job.wrap = egui::text::TextWrapping::truncate_at_width(place);
    let g = p.layout_job(job);
    p.galley(pos2(x, r.center().y - g.size().y / 2.0), g, encre);
    if rep.hovered() && !ouvert {
        bulle_de_l_evenement(ui, e, aujourdhui, cle);
    }
    let rep = rep.on_hover_cursor(egui::CursorIcon::PointingHand);
    menu_de_l_evenement(&rep, e, cle)
}

/// Le calendrier : le choix du tableau du suivi (« Tout », puis ceux qui ont quelque chose au calendrier dans cette
/// vue ; aucun choix sans eux, la maison seule), sa barre d'outils (le titre, ‹ Aujourd'hui ›, « Mois · Semaine »), le
/// mois ou la semaine, la légende des sortes montrées. Rend ce qu'on a choisi dans le menu d'un événement, et si le
/// tableau montré est celui des opérations (elles se demandent alors au chef).
pub(super) fn vue(ui: &mut egui::Ui, p: &Planning, vue: Vue, aujourdhui: i64) -> (Option<Action>, bool) {
    let avant = etat(ui.ctx());
    let mut e = avant.clone();
    let tableaux: Vec<(&str, &str)> = TABLEAUX
        .iter()
        .copied()
        .filter(|(cle, _)| p.evenements.iter().any(|x| se_montre(x.cote, vue) && x.tableau.as_deref() == Some(*cle)))
        .collect();
    // Le tableau choisi, s'il a quelque chose dans cette vue (la famille n'en a aucun) : sinon, tout.
    let mut tableau = e.tableau.clone().filter(|t| tableaux.iter().any(|(c, _)| c == t));
    if !tableaux.is_empty() {
        let titres: Vec<String> =
            std::iter::once("Tout".to_string()).chain(tableaux.iter().map(|(_, t)| t.to_string())).collect();
        let k = tableau.as_deref().and_then(|t| tableaux.iter().position(|(c, _)| *c == t)).map_or(0, |j| j + 1);
        let icone = super::ONGLETS[1].1;
        if let Some(j) = choix_du_tableau(ui, "tableau-du-calendrier", &titres, k, icone).filter(|j| *j != k) {
            tableau = (j > 0).then(|| tableaux[j - 1].0.to_string());
            e.tableau = tableau.clone();
            e.choisi = None;
        }
        ui.add_space(10.0);
    }
    let repere = e.repere.unwrap_or(aujourdhui);
    let places = places(p, vue, tableau.as_deref());
    let largeur = ui.available_width();
    let (barre, _) = ui.allocate_exact_size(vec2(largeur, 40.0), Sense::hover());
    let mut gauche = ui.new_child(UiBuilder::new().max_rect(barre).layout(Layout::left_to_right(Align::Center)));
    let (annee, numero_du_mois) = super::annee_et_mois(repere);
    let titre = match e.mode {
        Mode::Mois => format!("{} {annee}", theme::casse_de_phrase(&nom_du_mois(repere))),
        Mode::Semaine => titre_de_la_semaine(lundi(repere)),
    };
    titre_de_vue(&mut gauche, &titre);
    gauche.add_space(18.0);
    let (deja, aides) = match e.mode {
        Mode::Mois => (bornes_du_mois(repere) == bornes_du_mois(aujourdhui), ("Le mois d'avant", "Le mois d'après")),
        Mode::Semaine => (lundi(repere) == lundi(aujourdhui), ("La semaine d'avant", "La semaine d'après")),
    };
    if let Some(pas) = navigation(&mut gauche, "calendrier", aides, deja) {
        e.repere = match (pas, e.mode) {
            (0, _) => None,
            (s, Mode::Mois) => Some(premier_jour(annee, numero_du_mois + s)),
            (s, Mode::Semaine) => Some(lundi(repere) + 7 * s),
        };
        e.choisi = None;
    }
    let mut droite = ui.new_child(UiBuilder::new().max_rect(barre).layout(Layout::right_to_left(Align::Center)));
    let modes = [
        theme::Segment { icone: None, mot: "Mois", aide: "Le mois entier" },
        theme::Segment { icone: None, mot: "Semaine", aide: "La semaine, jour par jour" },
    ];
    let rang = if e.mode == Mode::Mois { 0 } else { 1 };
    if let Some(k) = theme::segments_erp(&mut droite, "mode-du-calendrier", &modes, rang, 34.0)
        && k != rang
    {
        e.mode = if k == 0 { Mode::Mois } else { Mode::Semaine };
    }
    ui.add_space(12.0);
    let (action, vers_la_semaine) = match e.mode {
        Mode::Mois => grille_du_mois(ui, p, &places, &mut e, aujourdhui),
        Mode::Semaine => (semaine(ui, p, &places, lundi(e.repere.unwrap_or(aujourdhui)), aujourdhui), None),
    };
    if let Some(l) = vers_la_semaine {
        e.mode = Mode::Semaine;
        e.repere = Some(l);
    }
    if e != avant {
        garder(ui.ctx(), e.clone());
    }
    // La légende : les sortes de ce qui se montre, dans l'ordre où elles se nomment.
    let (de, a) = etendue(&e, aujourdhui);
    let mut cles: Vec<Cle> = Vec::new();
    for pl in places.iter().filter(|pl| pl.fin >= de && pl.debut <= a) {
        let (teinte, icone, nom) = allure_de(&p.evenements[pl.k], aujourdhui);
        if !cles.iter().any(|c| c.nom == nom) {
            cles.push(Cle { couleur: teinte.vif[1], forme: FormeDeCle::Pastille, icone: Some(icone), nom: nom.into() });
        }
    }
    cles.sort_by_key(|c| ORDRE_DE_LA_LEGENDE.iter().position(|n| *n == c.nom).unwrap_or(usize::MAX));
    ui.add_space(10.0);
    if cles.is_empty() {
        let ou = match tableau.as_deref().and_then(|t| tableaux.iter().find(|(c, _)| *c == t)) {
            Some((_, titre)) => format!("dans « {titre} »"),
            None => "au calendrier".to_string(),
        };
        let rien = match e.mode {
            Mode::Mois => format!("Rien {ou} en {}.", nom_du_mois(e.repere.unwrap_or(aujourdhui))),
            Mode::Semaine => format!("Rien {ou} cette semaine-là."),
        };
        ui.label(egui::RichText::new(rien).size(13.0).color(erp::TEXTE_2));
    } else {
        legende(ui, &cles);
    }
    (action, tableau.as_deref() == Some("operations"))
}

/// L'ordre des sortes dans la légende : l'entreprise, puis la maison.
const ORDRE_DE_LA_LEGENDE: [&str; 14] = [
    "Chantiers", "Opérations", "Soumissions", "Ventes", "Livraisons", "Rendez-vous", "Notes de l'ERP", "Factures",
    "Factures échues", "Échéances", "École", "Anniversaires", "Rappels d'Eve", "Autres",
];

/// Les jours que montre le calendrier : la grille du mois (ses semaines entières), ou la semaine.
fn etendue(e: &Etat, aujourdhui: i64) -> (i64, i64) {
    let repere = e.repere.unwrap_or(aujourdhui);
    match e.mode {
        Mode::Mois => {
            let (premier, suivant) = bornes_du_mois(repere);
            (lundi(premier), lundi(suivant - 1) + 6)
        }
        Mode::Semaine => (lundi(repere), lundi(repere) + 6),
    }
}

/// Le titre d'une semaine : « 26 oct. – 1er nov. 2026 », « 2 – 8 nov. 2026 ».
fn titre_de_la_semaine(lundi: i64) -> String {
    let (d, f) = (centre::date_du_numero(lundi), centre::date_du_numero(lundi + 6));
    let annee = &f[..4];
    if d[5..7] == f[5..7] {
        format!("{} – {} {annee}", quantieme(lundi), centre::date_courte(&f))
    } else {
        format!("{} – {} {annee}", centre::date_courte(&d), centre::date_courte(&f))
    }
}

/// Le lavis des cases : un autre mois, un samedi ou un dimanche.
const AUTRE_MOIS: Color32 = Color32::from_rgb(0xF6, 0xF8, 0xFB);
const FIN_DE_SEMAINE: Color32 = Color32::from_rgb(0xFA, 0xFB, 0xFD);

/// Aujourd'hui allumé, comme dans les calendriers d'aujourd'hui (`r` : sa case du mois, ou sa colonne de la semaine) :
/// lavé de l'accent, cerclé d'un trait de l'accent, un halo pâle en dedans. Trois formes, à poser sous ses événements.
fn aujourdhui_allume(r: Rect, rayon: f32) -> [egui::Shape; 3] {
    let accent = erp::PRIMAIRE;
    [
        egui::Shape::rect_filled(r.shrink(1.0), rayon, accent.gamma_multiply(0.085)),
        egui::Shape::rect_stroke(
            r.shrink(3.0),
            (rayon - 1.5).max(0.0),
            Stroke::new(3.0, accent.gamma_multiply(0.13)),
            egui::StrokeKind::Inside,
        ),
        egui::Shape::rect_stroke(
            r.shrink(1.5),
            rayon,
            Stroke::new(1.5, accent.gamma_multiply(0.8)),
            egui::StrokeKind::Inside,
        ),
    ]
}

/// La hauteur d'une pastille, et ce qui les sépare.
const PASTILLE: f32 = 21.0;
const ENTRE: f32 = 3.0;

/// Le mois : l'en-tête des jours, puis ses semaines ; une case large, ses pastilles et ses bandes ; une case étroite,
/// ses points, et l'agenda du jour touché dessous. Rend ce qu'on a choisi dans un menu, et la semaine à ouvrir (un clic
/// sur « + 2 autres »).
fn grille_du_mois(
    ui: &mut egui::Ui,
    p: &Planning,
    places: &[Place],
    e: &mut Etat,
    aujourdhui: i64,
) -> (Option<Action>, Option<i64>) {
    let repere = e.repere.unwrap_or(aujourdhui);
    let (premier, suivant) = bornes_du_mois(repere);
    let (debut, fin) = (lundi(premier), lundi(suivant - 1) + 6);
    let semaines = ((fin - debut + 1) / 7) as usize;
    let (mut action, mut vers_la_semaine) = (None, None);
    let mut choisi = e.choisi;
    theme::carte(ui, None, 0.0, |ui| {
        let l = ui.available_width();
        let cw = l / 7.0;
        let compact = cw < 112.0;
        // Le jour de la semaine d'aujourd'hui s'allume dans l'en-tête quand la grille le montre.
        let colonne_d_aujourdhui = (debut..=fin).contains(&aujourdhui).then(|| jour_de_semaine(aujourdhui));
        entete_des_jours(ui, debut, cw, compact, colonne_d_aujourdhui);
        let rh = if compact { 54.0 } else { (cw * 0.76).clamp(112.0, 136.0) };
        let (grille, _) = ui.allocate_exact_size(vec2(l, rh * semaines as f32), Sense::hover());
        let pinceau = ui.painter().clone();
        for w in 0..semaines {
            let lundi_w = debut + 7 * w as i64;
            let y0 = grille.top() + rh * w as f32;
            if w > 0 {
                pinceau.hline(grille.x_range(), y0, Stroke::new(1.0, erp::FILET_DE_CARTE));
            }
            for j in 0..7 {
                let n = lundi_w + j;
                let case = Rect::from_min_size(pos2(grille.left() + cw * j as f32, y0), vec2(cw, rh));
                if j > 0 {
                    pinceau.vline(case.left(), case.y_range(), Stroke::new(1.0, erp::FILET_DE_CARTE));
                }
                let dans_le_mois = (premier..suivant).contains(&n);
                let fond = if !dans_le_mois {
                    Some(AUTRE_MOIS)
                } else if jour_de_semaine(n) >= 5 {
                    Some(FIN_DE_SEMAINE)
                } else {
                    None
                };
                if let Some(f) = fond {
                    pinceau.rect_filled(case.shrink(0.5), 0.0, f);
                }
                if n == aujourdhui {
                    pinceau.extend(aujourdhui_allume(case, if compact { 6.0 } else { 9.0 }));
                }
                if compact {
                    let rep = ui.interact(case, egui::Id::unique(("jour-du-calendrier", n)), Sense::click());
                    if rep.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        choisi = Some(n);
                    }
                    if choisi == Some(n) {
                        pinceau.rect_stroke(
                            case.shrink(2.0),
                            6.0,
                            Stroke::new(1.5, erp::PRIMAIRE),
                            egui::StrokeKind::Inside,
                        );
                    }
                }
                numero_du_jour(&pinceau, case, n, aujourdhui, dans_le_mois, compact);
                if compact {
                    points(&pinceau, case, p, places, n, aujourdhui);
                }
            }
            if !compact {
                let (a, s) = semaine_du_mois(ui, p, places, lundi_w, y0, grille.left(), cw, rh, aujourdhui);
                action = action.take().or(a);
                vers_la_semaine = vers_la_semaine.or(s);
            }
        }
        if compact {
            let jour = choisi
                .filter(|n| (premier..suivant).contains(n))
                .unwrap_or(if (premier..suivant).contains(&aujourdhui) { aujourdhui } else { premier });
            ui.add_space(12.0);
            action = action.take().or(agenda(ui, p, places, jour, jour, aujourdhui));
        }
    });
    e.choisi = choisi;
    (action, vers_la_semaine)
}

/// L'en-tête d'une grille de jours : « LUN », « MAR »… en petites capitales espacées, et un filet dessous ; le jour de
/// la semaine d'aujourd'hui (`aujourdhui` : sa colonne, 0 le lundi), quand la grille le montre, en gras de l'accent et
/// souligné d'un trait de l'accent.
fn entete_des_jours(ui: &mut egui::Ui, lundi: i64, cw: f32, compact: bool, aujourdhui: Option<i64>) {
    let l = cw * 7.0;
    let (tete, _) = ui.allocate_exact_size(vec2(l, 26.0), Sense::hover());
    let p = ui.painter();
    for j in 0..7 {
        let allume = aujourdhui == Some(j);
        let format = egui::text::TextFormat {
            font_id: FontId::new(11.0, FontFamily::Name(if allume { GRAS } else { SEMI }.into())),
            color: if allume { erp::PRIMAIRE } else { erp::TEXTE_2 },
            extra_letter_spacing: 1.2,
            ..Default::default()
        };
        let mot = jour_en_trois_lettres(lundi + j);
        let mot = if compact { mot.chars().take(1).collect() } else { mot };
        let mut job = egui::text::LayoutJob::default();
        job.append(&mot, 0.0, format);
        let g = p.layout_job(job);
        let centre = tete.left() + cw * (j as f32 + 0.5);
        let y = tete.center().y - g.size().y / 2.0 - 2.0;
        if allume {
            let trait_ = Rect::from_center_size(pos2(centre, tete.bottom() - 2.5), vec2(g.size().x.max(16.0), 3.0));
            p.rect_filled(trait_, 1.5, erp::PRIMAIRE);
        }
        p.galley(pos2(centre - g.size().x / 2.0, y), g, erp::TEXTE_2);
    }
    p.hline(tete.x_range(), tete.bottom() - 0.5, Stroke::new(1.0, erp::FILET_DE_CARTE));
}

/// Le numéro d'un jour dans sa case, en haut à gauche (au milieu, étroite) : aujourd'hui dans une pastille de l'accent,
/// le premier d'un mois avec son mois (« 1er nov. »), un autre mois pâli.
fn numero_du_jour(p: &egui::Painter, case: Rect, n: i64, aujourdhui: i64, dans_le_mois: bool, compact: bool) {
    let q = quantieme(n);
    let texte = if q == 1 && !compact { centre::date_courte(&centre::date_du_numero(n)) } else { q.to_string() };
    let encre = match (n == aujourdhui, dans_le_mois) {
        (true, _) => Color32::WHITE,
        (false, true) => erp::TITRE,
        (false, false) => erp::TEXTE_3,
    };
    let police = FontId::new(12.5, FontFamily::Name(if n == aujourdhui { GRAS } else { SEMI }.into()));
    let g = p.layout_no_wrap(texte, police, encre);
    let centre = if compact {
        pos2(case.center().x, case.top() + 16.0)
    } else {
        pos2(case.left() + 8.0 + g.size().x / 2.0, case.top() + 14.0)
    };
    if n == aujourdhui {
        let r = Rect::from_center_size(centre, vec2((g.size().x + 12.0).max(24.0), 24.0));
        p.rect_filled(r, 12.0, erp::PRIMAIRE);
    }
    p.galley(centre - g.size() / 2.0, g, encre);
}

/// Les points d'une case étroite : un par événement du jour (quatre au plus, puis « + »), de la teinte de sa sorte.
fn points(p: &egui::Painter, case: Rect, plan: &Planning, places: &[Place], n: i64, aujourdhui: i64) {
    let du_jour: Vec<&Place> = places.iter().filter(|pl| pl.debut <= n && pl.fin >= n).collect();
    if du_jour.is_empty() {
        return;
    }
    let montres = du_jour.len().min(4);
    let pas = 9.0;
    let x0 = case.center().x - pas * (montres as f32 - 1.0) / 2.0;
    let y = case.top() + 36.0;
    for (i, pl) in du_jour.iter().take(montres).enumerate() {
        let (teinte, _, _) = allure_de(&plan.evenements[pl.k], aujourdhui);
        p.circle_filled(pos2(x0 + pas * i as f32, y), 3.2, teinte.vif[1]);
    }
    if du_jour.len() > montres {
        let police = FontId::new(10.0, FontFamily::Name(SEMI.into()));
        p.text(pos2(x0 + pas * montres as f32 - 2.0, y), Align2::LEFT_CENTER, "+", police, erp::TEXTE_2);
    }
}

/// Une semaine du mois (de `lundi_w`, sa rangée en `y0`, ses cases de `cw` sur `rh`) : ses bandes, chacune dans son
/// couloir, puis les événements d'un jour dans les couloirs libres de leur case, par heure ; ce qui ne tient pas, « + 2
/// autres » en haut à droite de sa case (à côté de son numéro, il ne prend pas de place), qui ouvre la semaine. Rend ce
/// qu'on a choisi dans un menu, et la semaine à ouvrir.
#[allow(clippy::too_many_arguments)]
fn semaine_du_mois(
    ui: &mut egui::Ui,
    p: &Planning,
    places: &[Place],
    lundi_w: i64,
    y0: f32,
    gauche: f32,
    cw: f32,
    rh: f32,
    aujourdhui: i64,
) -> (Option<Action>, Option<i64>) {
    let dimanche = lundi_w + 6;
    let mut bandes: Vec<Place> =
        places.iter().filter(|pl| pl.bande() && pl.fin >= lundi_w && pl.debut <= dimanche).copied().collect();
    ranger_les_bandes(&mut bandes, lundi_w);
    let lignes = (((rh - 30.0) / (PASTILLE + ENTRE)).floor() as usize).max(1);
    let mut pris = vec![vec![false; lignes]; 7];
    let mut caches = [0usize; 7];
    let mut action = None;
    let y_de = |ligne: usize| y0 + 28.0 + (PASTILLE + ENTRE) * ligne as f32;
    for (b, c) in bandes.iter().zip(couloirs(&bandes, lundi_w)) {
        let (de, a) = ((b.debut.max(lundi_w) - lundi_w) as usize, (b.fin.min(dimanche) - lundi_w) as usize);
        if c >= lignes {
            for j in de..=a {
                caches[j] += 1;
            }
            continue;
        }
        for pris_du_jour in pris.iter_mut().take(a + 1).skip(de) {
            pris_du_jour[c] = true;
        }
        let r = Rect::from_min_max(
            pos2(gauche + cw * de as f32 + 4.0, y_de(c)),
            pos2(gauche + cw * (a + 1) as f32 - 4.0, y_de(c) + PASTILLE),
        );
        let coupe = (b.debut < lundi_w, b.fin > dimanche);
        let cle = format!("{}-{lundi_w}", p.evenements[b.k].id);
        let passe = b.fin < aujourdhui;
        action = action.take().or(pastille(ui, r, &p.evenements[b.k], aujourdhui, passe, coupe, &cle));
    }
    for j in 0..7 {
        let n = lundi_w + j as i64;
        let mut seules: Vec<&Place> = places.iter().filter(|pl| !pl.bande() && pl.debut == n).collect();
        seules.sort_by_key(|pl| ordre_du_jour(p, pl));
        for pl in seules {
            let Some(c) = (0..lignes).find(|c| !pris[j][*c]) else {
                caches[j] += 1;
                continue;
            };
            pris[j][c] = true;
            let r = Rect::from_min_size(pos2(gauche + cw * j as f32 + 4.0, y_de(c)), vec2(cw - 8.0, PASTILLE));
            let cle = format!("{}-{n}", p.evenements[pl.k].id);
            action = action.take().or(pastille(
                ui,
                r,
                &p.evenements[pl.k],
                aujourdhui,
                n < aujourdhui,
                (false, false),
                &cle,
            ));
        }
    }
    let mut vers_la_semaine = None;
    for (j, n) in caches.iter().enumerate().filter(|(_, n)| **n > 0) {
        let texte = if *n == 1 { "+ 1 autre".to_string() } else { format!("+ {n} autres") };
        let police = FontId::new(11.5, FontFamily::Name(SEMI.into()));
        let g = ui.painter().layout_no_wrap(texte, police, erp::PRIMAIRE);
        let taille = g.size() + vec2(8.0, 4.0);
        let r = Rect::from_min_size(pos2(gauche + cw * (j + 1) as f32 - 6.0 - taille.x, y0 + 4.0), taille);
        let rep = ui.interact(r, egui::Id::unique(("autres-du-jour", lundi_w, j)), Sense::click());
        if rep.hovered() {
            ui.painter().rect_filled(r.expand(2.0), 4.0, erp::SURVOL);
        }
        ui.painter().galley(r.min + vec2(4.0, 2.0), g, erp::PRIMAIRE);
        if rep.on_hover_text("Voir toute la semaine").on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            vers_la_semaine = Some(lundi_w);
        }
    }
    (action, vers_la_semaine)
}

/// La semaine : sept colonnes — l'en-tête de chaque jour (aujourd'hui dans une pastille de l'accent), ses bandes en
/// tête dans leurs couloirs, puis chaque jour ses événements en cartes (leur heure, leur titre, leur détail) ; étroite,
/// un agenda, jour après jour. Rend ce qu'on a choisi dans un menu.
fn semaine(ui: &mut egui::Ui, p: &Planning, places: &[Place], lundi_s: i64, aujourdhui: i64) -> Option<Action> {
    let mut action = None;
    theme::carte(ui, None, 0.0, |ui| {
        let l = ui.available_width();
        if l < 640.0 {
            action = agenda(ui, p, places, lundi_s, lundi_s + 6, aujourdhui);
            return;
        }
        let cw = l / 7.0;
        let dimanche = lundi_s + 6;
        // L'en-tête : le jour en trois lettres, son numéro en grand.
        let (tete, _) = ui.allocate_exact_size(vec2(l, 58.0), Sense::hover());
        let pinceau = ui.painter().clone();
        // Aujourd'hui allumé, sa colonne entière : posé sous tout le reste, sa hauteur ne se connaît qu'à la fin.
        let sous_tout: Vec<egui::layers::ShapeIdx> = (0..3).map(|_| pinceau.add(egui::Shape::Noop)).collect();
        for j in 0..7 {
            let n = lundi_s + j;
            let x = tete.left() + cw * (j as f32 + 0.5);
            let format = egui::text::TextFormat {
                font_id: FontId::new(11.0, FontFamily::Name(SEMI.into())),
                color: if n == aujourdhui { erp::PRIMAIRE } else { erp::TEXTE_2 },
                extra_letter_spacing: 1.2,
                ..Default::default()
            };
            let mut job = egui::text::LayoutJob::default();
            job.append(&jour_en_trois_lettres(n), 0.0, format);
            let g = pinceau.layout_job(job);
            pinceau.galley(pos2(x - g.size().x / 2.0, tete.top() + 2.0), g, erp::TEXTE_2);
            let encre = if n == aujourdhui {
                Color32::WHITE
            } else if n < aujourdhui {
                erp::TEXTE_2
            } else {
                erp::TITRE
            };
            let g = pinceau.layout_no_wrap(
                quantieme(n).to_string(),
                FontId::new(20.0, FontFamily::Name(GRAS.into())),
                encre,
            );
            let c = pos2(x, tete.top() + 36.0);
            if n == aujourdhui {
                pinceau.circle_filled(c, 17.0, erp::PRIMAIRE);
            }
            pinceau.galley(c - g.size() / 2.0, g, encre);
        }
        pinceau.hline(tete.x_range(), tete.bottom() - 0.5, Stroke::new(1.0, erp::FILET_DE_CARTE));
        // Les bandes, dans leurs couloirs.
        let mut bandes: Vec<Place> =
            places.iter().filter(|pl| pl.bande() && pl.fin >= lundi_s && pl.debut <= dimanche).copied().collect();
        ranger_les_bandes(&mut bandes, lundi_s);
        let couloirs = couloirs(&bandes, lundi_s);
        let n_couloirs = couloirs.iter().max().map_or(0, |c| c + 1);
        let h_bandes = if n_couloirs > 0 { 8.0 + (PASTILLE + ENTRE) * n_couloirs as f32 } else { 0.0 };
        let (zone_des_bandes, _) = ui.allocate_exact_size(vec2(l, h_bandes), Sense::hover());
        for (b, c) in bandes.iter().zip(&couloirs) {
            let (de, a) = ((b.debut.max(lundi_s) - lundi_s) as f32, (b.fin.min(dimanche) - lundi_s) as f32);
            let y = zone_des_bandes.top() + 6.0 + (PASTILLE + ENTRE) * *c as f32;
            let r = Rect::from_min_max(
                pos2(tete.left() + cw * de + 4.0, y),
                pos2(tete.left() + cw * (a + 1.0) - 4.0, y + PASTILLE),
            );
            let coupe = (b.debut < lundi_s, b.fin > dimanche);
            let cle = format!("{}-semaine-{lundi_s}", p.evenements[b.k].id);
            action = action.take().or(pastille(ui, r, &p.evenements[b.k], aujourdhui, b.fin < aujourdhui, coupe, &cle));
        }
        // Chaque jour, ses événements en cartes : mis en page d'abord, pour connaître la hauteur de la semaine.
        let largeur_de_carte = cw - 10.0;
        let jours: Vec<Vec<(usize, CarteDuJour)>> = (0..7)
            .map(|j| {
                let n = lundi_s + j;
                let mut seules: Vec<&Place> = places.iter().filter(|pl| !pl.bande() && pl.debut == n).collect();
                seules.sort_by_key(|pl| ordre_du_jour(p, pl));
                seules
                    .iter()
                    .map(|pl| (pl.k, CarteDuJour::mise_en_page(&pinceau, &p.evenements[pl.k], largeur_de_carte)))
                    .collect()
            })
            .collect();
        let hauteur = jours
            .iter()
            .map(|cartes| cartes.iter().map(|(_, c)| c.hauteur + 6.0).sum::<f32>())
            .fold(0.0, f32::max)
            .max(300.0)
            + 8.0;
        let (corps, _) = ui.allocate_exact_size(vec2(l, hauteur), Sense::hover());
        for j in 1..7 {
            let x = corps.left() + cw * j as f32;
            pinceau.vline(x, tete.top() + 50.0..=corps.bottom(), Stroke::new(1.0, erp::FILET_DE_CARTE));
        }
        for (j, cartes) in jours.into_iter().enumerate() {
            let n = lundi_s + j as i64;
            if n == aujourdhui {
                let colonne = Rect::from_min_max(
                    pos2(corps.left() + cw * j as f32 + 1.0, tete.top() - 4.0),
                    pos2(corps.left() + cw * (j + 1) as f32 - 1.0, corps.bottom()),
                );
                for (i, forme) in sous_tout.iter().zip(aujourdhui_allume(colonne, 12.0)) {
                    pinceau.set(*i, forme);
                }
            }
            let mut y = corps.top() + 6.0;
            for (k, c) in cartes {
                let r =
                    Rect::from_min_size(pos2(corps.left() + cw * j as f32 + 5.0, y), vec2(largeur_de_carte, c.hauteur));
                y += c.hauteur + 6.0;
                let cle = format!("{}-jour-{n}", p.evenements[k].id);
                action = action.take().or(c.peindre(ui, r, &p.evenements[k], aujourdhui, n < aujourdhui, &cle));
            }
        }
    });
    action
}

/// Une carte d'un événement dans la semaine, mise en page : son heure (ou « Journée »), son titre, son détail.
struct CarteDuJour {
    heure: std::sync::Arc<egui::Galley>,
    titre: std::sync::Arc<egui::Galley>,
    detail: Option<std::sync::Arc<egui::Galley>>,
    hauteur: f32,
}

impl CarteDuJour {
    fn mise_en_page(p: &egui::Painter, e: &Evenement, largeur: f32) -> CarteDuJour {
        let dedans = (largeur - 22.0).max(30.0);
        let (heure, _) = libelle(e);
        let heure = if heure.is_empty() { "Journée".to_string() } else { heure };
        let semi = FontId::new(11.5, FontFamily::Name(SEMI.into()));
        let heure = p.layout_no_wrap(heure, semi, erp::TEXTE_2);
        let mut job =
            egui::text::LayoutJob::simple(e.titre.trim().to_string(), FontId::proportional(12.5), erp::TITRE, dedans);
        job.wrap.max_rows = 3;
        let titre = p.layout_job(job);
        let detail = (!e.detail.trim().is_empty()).then(|| {
            let mut job = egui::text::LayoutJob::simple(
                e.detail.trim().to_string(),
                FontId::proportional(11.5),
                erp::TEXTE_2,
                dedans,
            );
            job.wrap.max_rows = 2;
            p.layout_job(job)
        });
        let hauteur =
            8.0 + heure.size().y + 2.0 + titre.size().y + detail.as_ref().map_or(0.0, |d| d.size().y + 2.0) + 8.0;
        CarteDuJour { heure, titre, detail, hauteur }
    }

    /// La carte dans `r` : un fond lavé de la teinte de sa sorte, son liseré à gauche, son icône et son heure, son
    /// titre, son détail ; plus pâle un jour passé ; sa bulle au survol, son menu au clic.
    fn peindre(
        self,
        ui: &mut egui::Ui,
        r: Rect,
        e: &Evenement,
        aujourdhui: i64,
        passe: bool,
        cle: &str,
    ) -> Option<Action> {
        let rep = ui.interact(r, egui::Id::unique(("evenement-du-calendrier", cle)), Sense::click());
        let ouvert = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&rep));
        let (teinte, icone, _) = allure_de(e, aujourdhui);
        let p = ui.painter().clone();
        let force = if rep.hovered() || ouvert {
            0.36
        } else if passe {
            0.12
        } else {
            0.2
        };
        p.rect_filled(r, 8.0, teinte.pastel.gamma_multiply(force));
        let lisere = Rect::from_min_size(r.min, vec2(3.0, r.height()));
        p.rect_filled(
            lisere,
            CornerRadius { nw: 8, sw: 8, ne: 0, se: 0 },
            teinte.vif[1].gamma_multiply(if passe { 0.55 } else { 1.0 }),
        );
        let x = r.left() + 12.0;
        let mut y = r.top() + 8.0;
        let police = FontId::new(11.0, FontFamily::Name(ICONES.into()));
        p.text(pos2(x + 5.0, y + self.heure.size().y / 2.0), Align2::CENTER_CENTER, icone, police, encre_de(teinte));
        let h = self.heure.size().y;
        p.galley(pos2(x + 15.0, y), self.heure, erp::TEXTE_2);
        y += h + 2.0;
        let encre = if passe { erp::TEXTE_2 } else { erp::TITRE };
        let h = self.titre.size().y;
        p.galley(pos2(x, y), self.titre, encre);
        y += h + 2.0;
        if let Some(d) = self.detail {
            p.galley(pos2(x, y), d, erp::TEXTE_2);
        }
        if rep.hovered() && !ouvert {
            bulle_de_l_evenement(ui, e, aujourdhui, cle);
        }
        let rep = rep.on_hover_cursor(egui::CursorIcon::PointingHand);
        menu_de_l_evenement(&rep, e, cle)
    }
}

/// Un agenda, jour après jour (de `de` à `a`) : la date de chaque jour qui a des événements (aujourd'hui, à l'accent),
/// puis chacun sur sa rangée — sa pastille d'icône, son heure, son titre et son détail. Rend ce qu'on a choisi dans un
/// menu.
fn agenda(ui: &mut egui::Ui, p: &Planning, places: &[Place], de: i64, a: i64, aujourdhui: i64) -> Option<Action> {
    let mut action = None;
    let mut vide = true;
    for n in de..=a {
        let mut du_jour: Vec<&Place> = places.iter().filter(|pl| pl.debut <= n && pl.fin >= n).collect();
        if du_jour.is_empty() {
            continue;
        }
        vide = false;
        du_jour.sort_by_key(|pl| (!pl.bande(), ordre_du_jour(p, pl)));
        let date = theme::casse_de_phrase(&crate::ambiance::date_longue(&centre::date_du_numero(n)));
        let date = if n == aujourdhui { format!("Aujourd'hui · {date}") } else { date };
        let encre = if n == aujourdhui { erp::PRIMAIRE } else { erp::TITRE };
        ui.add_space(4.0);
        ui.label(egui::RichText::new(date).family(FontFamily::Name(SEMI.into())).size(13.5).color(encre));
        for pl in du_jour {
            let e = &p.evenements[pl.k];
            action =
                action.take().or(rangee_d_agenda(ui, e, aujourdhui, n < aujourdhui, &format!("{}-agenda-{n}", e.id)));
        }
        ui.add_space(6.0);
    }
    if vide {
        let rien = if de == a { "Rien au calendrier ce jour-là." } else { "Rien au calendrier cette semaine-là." };
        ui.label(egui::RichText::new(rien).size(13.0).color(erp::TEXTE_2));
    }
    action
}

/// Une rangée de l'agenda : la pastille de l'icône de sa sorte, son heure (ou « Journée »), son titre, son détail
/// dessous ; un lavis au survol ; sa bulle, son menu.
fn rangee_d_agenda(ui: &mut egui::Ui, e: &Evenement, aujourdhui: i64, passe: bool, cle: &str) -> Option<Action> {
    let largeur = ui.available_width();
    let (teinte, icone, _) = allure_de(e, aujourdhui);
    let p = ui.painter().clone();
    let (heure, titre) = libelle(e);
    let heure = if heure.is_empty() { "Journée".to_string() } else { heure };
    let x_texte = 34.0 + 64.0;
    let titre = p.layout(
        titre,
        FontId::proportional(13.5),
        if passe { erp::TEXTE_2 } else { erp::TITRE },
        (largeur - x_texte).max(60.0),
    );
    let detail = (!e.detail.trim().is_empty()).then(|| {
        p.layout(e.detail.trim().to_string(), FontId::proportional(12.0), erp::TEXTE_2, (largeur - x_texte).max(60.0))
    });
    let h = (titre.size().y + detail.as_ref().map_or(0.0, |d| d.size().y + 1.0)).max(26.0) + 8.0;
    let (r, _) = ui.allocate_exact_size(vec2(largeur, h), Sense::hover());
    let rep = ui.interact(r, egui::Id::unique(("evenement-du-calendrier", cle)), Sense::click());
    let ouvert = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&rep));
    if rep.hovered() || ouvert {
        p.rect_filled(r.expand2(vec2(6.0, 0.0)), 8.0, erp::SURVOL);
    }
    let case = Rect::from_min_size(pos2(r.left(), r.top() + 4.0), vec2(24.0, 24.0));
    theme::pastille(&p, case, icone, teinte);
    let semi = FontId::new(12.5, FontFamily::Name(SEMI.into()));
    p.text(pos2(r.left() + 34.0, r.top() + 4.0 + 12.0), Align2::LEFT_CENTER, heure, semi, erp::TEXTE_2);
    let h_titre = titre.size().y;
    p.galley(pos2(r.left() + x_texte, r.top() + 5.0), titre, erp::TITRE);
    if let Some(d) = detail {
        p.galley(pos2(r.left() + x_texte, r.top() + 6.0 + h_titre), d, erp::TEXTE_2);
    }
    if rep.hovered() && !ouvert {
        bulle_de_l_evenement(ui, e, aujourdhui, cle);
    }
    let rep = rep.on_hover_cursor(egui::CursorIcon::PointingHand);
    menu_de_l_evenement(&rep, e, cle)
}

#[cfg(test)]
mod essais {
    use super::*;
    use crate::planning::{Cote, Sorte};

    fn n(d: &str) -> i64 {
        centre::numero_du_jour(d).unwrap()
    }

    fn evenement(debut: &str, fin: Option<&str>, heure: Option<&str>, titre: &str, cote: Cote) -> Evenement {
        Evenement {
            id: format!("{debut}·{titre}"),
            debut: debut.into(),
            fin: fin.map(Into::into),
            heure: heure.map(Into::into),
            titre: titre.into(),
            detail: String::new(),
            sorte: Sorte::RendezVous,
            cote,
            tableau: None,
            source: String::new(),
        }
    }

    #[test]
    fn la_grille_d_un_mois_va_du_lundi_au_dimanche() {
        let octobre = Etat { mode: Mode::Mois, repere: Some(n("2026-10-30")), choisi: None, tableau: None };
        assert_eq!(etendue(&octobre, 0), (n("2026-09-28"), n("2026-11-01")), "cinq semaines");
        let novembre = Etat { repere: Some(n("2026-11-15")), ..octobre };
        let (de, a) = etendue(&novembre, 0);
        assert_eq!((de, a), (n("2026-10-26"), n("2026-12-06")), "le 1er novembre, un dimanche : six semaines");
        assert_eq!((a - de + 1) / 7, 6);
        let semaine = Etat { mode: Mode::Semaine, ..Etat::default() };
        assert_eq!(etendue(&semaine, n("2026-10-30")), (n("2026-10-26"), n("2026-11-01")));
        assert_eq!(titre_de_la_semaine(n("2026-10-26")), "26 oct. – 1er nov. 2026");
        assert_eq!(titre_de_la_semaine(n("2026-11-02")), "2 – 8 nov. 2026");
    }

    #[test]
    fn les_bandes_prennent_le_premier_couloir_libre() {
        let lundi = n("2026-10-26");
        let b = |k: usize, de: i64, a: i64| Place { k, debut: lundi + de, fin: lundi + a };
        // Du lundi au mercredi, du mardi au jeudi (elle chevauche la première), du jeudi au samedi (la première est
        // finie), une de la semaine d'avant à mardi.
        let bandes = [b(0, -3, 1), b(1, 0, 2), b(2, 1, 3), b(3, 3, 5)];
        assert_eq!(couloirs(&bandes, lundi), [0, 1, 2, 0]);
        // Deux bandes venues de la semaine d'avant, qui finissent le même jour : celle qui a commencé avant garde le
        // premier couloir, comme la semaine d'avant (le planning les donne dans l'autre ordre).
        let mut venues = [b(0, -10, 4), b(1, -24, 4), b(2, 2, 3)];
        ranger_les_bandes(&mut venues, lundi);
        assert_eq!(venues.map(|v| v.k), [1, 0, 2]);
        ranger_les_bandes(&mut venues, lundi - 7);
        assert_eq!(venues.map(|v| v.k), [1, 0, 2], "la semaine d'avant : la même");
    }

    #[test]
    fn un_evenement_se_place_dans_ses_jours_et_sa_vue() {
        let p = Planning {
            evenements: vec![
                evenement("2026-10-30", None, Some("17:30"), "Hockey", Cote::Famille),
                evenement("2026-11-02", Some("2026-11-06"), None, "Charpente", Cote::Entreprise),
                evenement("2026-11-02", Some("2026-10-01"), None, "Une fin avant le début", Cote::Entreprise),
                evenement("bientôt", None, None, "Une date qui ne se lit pas", Cote::Entreprise),
            ],
            ..Planning::default()
        };
        let toutes = places(&p, Vue::Tout, None);
        assert_eq!(toutes.len(), 3, "la date qui ne se lit pas s'en va");
        assert!(toutes[1].bande() && !toutes[0].bande() && !toutes[2].bande(), "une fin avant le début : un jour");
        assert_eq!(places(&p, Vue::Famille, None).len(), 1);
        assert_eq!(places(&p, Vue::Entreprise, None).len(), 2);
        assert_eq!(libelle(&p.evenements[0]), ("17 h 30".to_string(), "Hockey".to_string()));
        // Le choix d'un tableau du suivi : ses événements seuls.
        let mut p = p;
        p.evenements[1].tableau = Some("bons_travail".into());
        let du_tableau = places(&p, Vue::Tout, Some("bons_travail"));
        assert_eq!(du_tableau.iter().map(|pl| pl.k).collect::<Vec<_>>(), [1]);
        assert!(places(&p, Vue::Tout, Some("factures")).is_empty());
    }

    /// Un mois dessiné sans fenêtre : un clic sur la pastille d'un événement ouvre son menu, et « En parler à Eve »
    /// rend la demande, sa source comprise ; « + 1 autre » ouvre la semaine.
    #[test]
    fn un_evenement_du_mois_ouvre_son_menu() {
        let ctx = egui::Context::default();
        crate::natif::polices(&ctx);
        let aujourdhui = n("2026-10-30");
        let mut hockey = evenement("2026-10-30", None, Some("17:30"), "Hockey de Félix", Cote::Famille);
        hockey.source = "le hockey de Félix, à l'aréna".into();
        let mut p = Planning { evenements: vec![hockey], ..Planning::default() };
        for h in ["08:00", "09:00", "10:00", "11:00", "12:00", "13:00"] {
            p.evenements.push(evenement("2026-10-29", None, Some(h), "Une réunion", Cote::Entreprise));
        }
        let mut t = 0.0;
        let mut image = |evenements: Vec<egui::Event>| {
            t += 0.1;
            let entree = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1100.0, 1000.0))),
                time: Some(t),
                events: evenements,
                ..Default::default()
            };
            let mut action = None;
            let _ = ctx.run_ui(entree, |ui| action = vue(ui, &p, Vue::Tout, aujourdhui).0);
            action
        };
        let bouton = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        assert_eq!(image(Vec::new()), None);
        let cle = format!("{}-{aujourdhui}", p.evenements[0].id);
        let pastille = ctx
            .read_response(egui::Id::unique(("evenement-du-calendrier", cle.as_str())))
            .expect("la pastille du hockey");
        let c = pastille.rect.center();
        image(vec![egui::Event::PointerMoved(c)]);
        image(vec![bouton(c, true)]);
        assert_eq!(image(vec![bouton(c, false)]), None, "un clic ouvre le menu");
        image(Vec::new());
        image(Vec::new());
        let parler = ctx.read_response(egui::Id::unique(("ligne-du-planning", cle.as_str(), 0usize)));
        let parler = parler.expect("en parler").rect.center();
        image(vec![egui::Event::PointerMoved(parler)]);
        image(vec![bouton(parler, true)]);
        let rendu = image(vec![bouton(parler, false)]);
        let Some(Action::Demander(d)) = rendu.clone() else {
            panic!("une demande : {rendu:?} (la ligne : {parler:?})")
        };
        assert!(d.contains("« Hockey de Félix », ven. 30 oct. à 17 h 30") && d.contains("à l'aréna"), "{d}");
        // Le jeudi a six réunions : sa case n'en tient que trois ou quatre, et « + N autres » ouvre la semaine.
        let autres =
            (0..7usize).find_map(|j| ctx.read_response(egui::Id::unique(("autres-du-jour", n("2026-10-26"), j))));
        let autres = autres.expect("« + N autres » le jeudi").rect.center();
        image(vec![egui::Event::PointerMoved(autres)]);
        image(vec![bouton(autres, true)]);
        image(vec![bouton(autres, false)]);
        assert_eq!(etat(&ctx).mode, Mode::Semaine);
        assert_eq!(etat(&ctx).repere, Some(n("2026-10-26")));
    }
}
