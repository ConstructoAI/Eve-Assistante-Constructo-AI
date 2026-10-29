//! Le Kanban du tableau de bord (l'onglet « Kanban ») : un tableau du suivi de Constructo AI, au choix (les chantiers,
//! les soumissions, les bons de travail, les ventes…), comme les tableaux de suivi d'aujourd'hui. Ses colonnes dans
//! l'ordre de l'ERP, chacune un couloir de verre sur le fond qui respire : en tête, son point (la couleur de ce qu'elle
//! dit : faite en vert, refusée en rouge, qui attend en ambre, en marche en bleu, pas commencée en gris), son titre et
//! combien de cartes l'ERP y compte ; puis ses cartes, blanches — leur titre, leur détail, leur montant, leur échéance
//! (rouge passée, ambre dans la semaine), les initiales de leurs responsables —, et « … et 11 autres dans Constructo
//! AI » quand l'ERP en compte plus qu'Eve n'en a lu. Sa règle (`garde` : « Statut géré via le Bon de travail ») sous le
//! choix du tableau. Trop de colonnes pour la place : elles défilent de côté. Un clic sur une carte ouvre son menu : en
//! parler, me le rappeler demain, et « Déplacer vers… » chacune des autres colonnes — une demande à Eve. Les statuts,
//! les titres et la règle viennent de l'ERP sans accents : ils se lisent en français à l'écran ([`en_francais`]), et
//! partent tels quels dans les demandes.

use std::sync::Arc;

use eframe::egui::{self, Align2, Color32, FontFamily, FontId, Rect, Sense, Stroke, StrokeKind, pos2, vec2};

use super::{
    Action, choix_du_tableau, couleur_de_la_personne, deplacer_la_carte, en_francais, initiales, menu_de_chose,
    parler_de_la_carte, rappel_de_la_carte, titre_de_vue,
};
use crate::centre;
use crate::natif::theme::{self, erp};
use crate::natif::{ICONES, SEMI};
use crate::planning::{Carte, Colonne, Planning, Tableau};

/// Ce que montre le Kanban : la clé du tableau choisi (vide : le premier).
#[derive(Clone, Debug, Default, PartialEq)]
struct Etat {
    tableau: String,
}

const ETAT: &str = "etat-du-kanban";

fn etat(ctx: &egui::Context) -> Etat {
    ctx.data(|d| d.get_temp::<Etat>(egui::Id::unique(ETAT))).unwrap_or_default()
}

fn garder(ctx: &egui::Context, e: Etat) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::unique(ETAT), e));
}

/// Le Kanban d'une scène de l'essai du thème : ce tableau (sa clé).
pub(super) fn preparer(ctx: &egui::Context, tableau: &str) {
    garder(ctx, Etat { tableau: tableau.to_string() });
}

/// La couleur de ce que dit une colonne, d'après son statut et son titre (sans égard aux accents ni à la casse) : ce
/// qui est fait, refusé, en attente, en marche, pas commencé ; sinon l'indigo d'un statut que rien ne dit.
fn couleur_de_la_colonne(c: &Colonne) -> erp::Teinte {
    let s = eve::parole::normaliser(&format!("{} {}", c.statut, c.titre));
    let a = |mots: &[&str]| mots.iter().any(|m| s.contains(m));
    if a(&["accept", "gagn", "termin", "complet", "ferme", "paye", "livre", "fait", "approuv"]) {
        erp::VERT
    } else if a(&["refus", "perdu", "annul", "rejet"]) {
        erp::ROUGE
    } else if a(&["attente", "pause", "suspend", "bloqu", "retard"]) {
        erp::AMBRE
    } else if a(&["cours", "envoy", "negoci", "propos", "actif", "production", "soumis", "valid"]) {
        erp::BLEU
    } else if a(&["brouillon", "planif", "a faire", "nouveau", "prospect", "qualif", "ouvert", "a venir"]) {
        erp::GRIS
    } else {
        erp::INDIGO
    }
}

/// L'échéance d'une carte, dite dans sa pastille (« 5 nov. ») et sa teinte : passée, le rouge ; dans la semaine,
/// l'ambre ; plus tard, le gris. Dans une colonne close (`close` : faite ou refusée), ce n'est plus qu'une date, en
/// gris : ce qui est fini n'est pas en retard. `None` sans échéance lisible.
fn echeance(c: &Carte, aujourdhui: i64, close: bool) -> Option<(String, erp::Teinte, &'static str)> {
    let date = c.echeance.as_deref()?;
    let n = centre::numero_du_jour(date)?;
    let (teinte, quand) = match n - aujourdhui {
        _ if close => (erp::GRIS, "close"),
        d if d < 0 => (erp::ROUGE, "échue"),
        0 => (erp::AMBRE, "aujourd'hui"),
        d if d <= 7 => (erp::AMBRE, "cette semaine"),
        _ => (erp::GRIS, "à venir"),
    };
    Some((centre::date_courte(date), teinte, quand))
}

/// Une colonne close : ce qu'elle dit est fait, ou refusé.
fn close(c: &Colonne) -> bool {
    let teinte = couleur_de_la_colonne(c);
    teinte == erp::VERT || teinte == erp::ROUGE
}

/// Un montant de carte : « 1,45 M$ » dès le million, sinon « 96 400 $ ».
fn montant(v: f64) -> String {
    if v.abs() >= 999_995.0 {
        let m = format!("{:.2}", v / 1_000_000.0);
        format!("{}\u{a0}M$", m.trim_end_matches('0').trim_end_matches('.').replace('.', ","))
    } else {
        format!("{}\u{a0}$", centre::milliers(v))
    }
}

/// Le pied d'une colonne : ce que l'ERP y compte de plus qu'Eve n'en a lu (« … et 11 autres dans Constructo AI ») ;
/// rien de lu, ce qu'il y compte (« 9 cartes dans Constructo AI » : les chantiers finis, qu'Eve ne lit pas) ; vide,
/// « Aucune carte ».
fn pied_de_colonne(c: &Colonne) -> Option<String> {
    let n = c.total.saturating_sub(c.cartes.len());
    match (c.cartes.is_empty(), n) {
        (true, 0) => Some("Aucune carte".into()),
        (true, 1) => Some("1 carte dans Constructo AI".into()),
        (true, n) => Some(format!("{n} cartes dans Constructo AI")),
        (false, 0) => None,
        (false, 1) => Some("… et 1 autre dans Constructo AI".into()),
        (false, n) => Some(format!("… et {n} autres dans Constructo AI")),
    }
}

/// Le tableau des opérations du suivi (sa clé ou son titre le disent) : ses cartes ne se lisent qu'à la demande.
fn des_operations(t: &Tableau) -> bool {
    [&t.cle, &t.titre].iter().any(|x| eve::parole::normaliser(x).contains("operation"))
}

/// La largeur d'un couloir : la place divisée entre les colonnes, de 240 à 320 points ; en deçà, elles défilent de côté
/// à 264.
fn largeur_des_couloirs(n: usize, l: f32) -> (f32, bool) {
    let ecart = 14.0;
    let n = n.max(1) as f32;
    let chacune = (l - ecart * (n - 1.0)) / n;
    if chacune >= 240.0 { (chacune.min(320.0), false) } else { (264.0, true) }
}

/// Le Kanban : le choix du tableau (en segments, ou un menu s'ils ne tiennent pas), son titre et sa règle, puis ses
/// colonnes. Rend ce qu'on a choisi dans le menu d'une carte, et si le tableau montré est celui des opérations (elles
/// se demandent alors au chef).
pub(super) fn vue(ui: &mut egui::Ui, p: &Planning, aujourdhui: i64) -> (Option<Action>, bool) {
    if p.tableaux.is_empty() {
        let texte = "Constructo AI n'a encore aucun tableau de suivi à montrer : ses chantiers, ses soumissions et ses \
                     bons de travail s'y rangeront, colonne par colonne.";
        super::etat_vide(ui, super::ONGLETS[3].1, erp::BLEU, "Aucun tableau de suivi", texte, None);
        return (None, false);
    }
    let avant = etat(ui.ctx());
    let k = p.tableaux.iter().position(|t| t.cle == avant.tableau).unwrap_or(0);
    let largeur = ui.available_width();
    let titres: Vec<String> = p.tableaux.iter().map(|t| en_francais(&t.titre)).collect();
    let choisi = choix_du_tableau(ui, "tableau-du-kanban", &titres, k, super::ONGLETS[3].1);
    if let Some(j) = choisi.filter(|j| *j != k) {
        garder(ui.ctx(), Etat { tableau: p.tableaux[j].cle.clone() });
    }
    let montre = choisi.unwrap_or(k);
    let t = &p.tableaux[montre];
    ui.add_space(14.0);
    // Le titre du tableau, combien de cartes il compte ; sa règle dessous.
    let total: usize = t.colonnes.iter().map(|c| c.total.max(c.cartes.len())).sum();
    ui.horizontal(|ui| {
        titre_de_vue(ui, &titres[montre]);
        ui.add_space(8.0);
        let compte =
            if total == 1 { "1 carte".to_string() } else { format!("{} cartes", centre::milliers(total as f64)) };
        ui.label(egui::RichText::new(compte).size(14.0).color(erp::TEXTE_2));
    });
    if let Some(g) = t.garde.as_deref().filter(|g| !g.trim().is_empty()) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let police = FontId::new(13.0, FontFamily::Name(ICONES.into()));
            ui.label(egui::RichText::new(theme::pictos::INFO.to_string()).font(police).color(erp::PRIMAIRE));
            ui.label(egui::RichText::new(en_francais(g)).size(13.0).color(erp::TEXTE_2));
        });
    }
    ui.add_space(12.0);
    if t.colonnes.is_empty() {
        let texte = "Ce tableau n'a pas encore de colonnes dans Constructo AI.";
        super::etat_vide(ui, super::ONGLETS[3].1, erp::BLEU, &titres[montre], texte, None);
        return (None, des_operations(t));
    }
    let (couloir, defile) = largeur_des_couloirs(t.colonnes.len(), largeur);
    let mut action = None;
    if defile {
        let sortie = egui::ScrollArea::horizontal()
            .id_salt(("couloirs-du-kanban", t.cle.as_str()))
            .auto_shrink([false, true])
            .show(ui, |ui| {
                action = couloirs(ui, t, couloir, aujourdhui);
            });
        // Les colonnes qui continuent de côté s'estompent dans le fond : il y en a d'autres à voir.
        let r = sortie.inner_rect;
        let avant = sortie.state.clamped_offset().x;
        let apres = sortie.content_size.x - r.width() - avant;
        let large = 40.0_f32.min(r.width() / 4.0);
        if avant > 1.0 {
            let bord = Rect::from_min_size(r.min, vec2(large, r.height()));
            theme::fondu_du_bord(ui.painter(), bord, theme::Bord::Gauche);
        }
        if apres > 1.0 {
            let bord = Rect::from_min_max(pos2(r.right() - large, r.top()), r.max);
            theme::fondu_du_bord(ui.painter(), bord, theme::Bord::Droite);
        }
    } else {
        action = couloirs(ui, t, couloir, aujourdhui);
    }
    (action, des_operations(t))
}

/// Une carte du Kanban, mise en page dans la largeur de son couloir : son titre, son détail, son pied (son montant, son
/// échéance, ses responsables).
struct CarteMiseEnPage {
    titre: Arc<egui::Galley>,
    detail: Option<Arc<egui::Galley>>,
    montant: Option<Arc<egui::Galley>>,
    echeance: Option<(Arc<egui::Galley>, erp::Teinte, &'static str)>,
    hauteur: f32,
}

const MARGE_DE_CARTE: f32 = 12.0;

impl CarteMiseEnPage {
    fn nouvelle(p: &egui::Painter, c: &Carte, largeur: f32, aujourdhui: i64, close: bool) -> CarteMiseEnPage {
        let dedans = (largeur - 2.0 * MARGE_DE_CARTE).max(40.0);
        let mut job = egui::text::LayoutJob::simple(
            c.titre.trim().to_string(),
            FontId::new(13.5, FontFamily::Name(SEMI.into())),
            erp::TITRE,
            dedans,
        );
        job.wrap.max_rows = 2;
        let titre = p.layout_job(job);
        let detail = (!c.detail.trim().is_empty()).then(|| {
            let mut job = egui::text::LayoutJob::simple(
                c.detail.trim().to_string(),
                FontId::proportional(12.5),
                erp::TEXTE_2,
                dedans,
            );
            job.wrap.max_rows = 2;
            p.layout_job(job)
        });
        let montant = c
            .montant
            .map(|m| p.layout_no_wrap(montant(m), FontId::new(13.0, FontFamily::Name(SEMI.into())), erp::TITRE));
        let echeance = echeance(c, aujourdhui, close).map(|(date, teinte, quand)| {
            (p.layout_no_wrap(date, FontId::new(11.5, FontFamily::Name(SEMI.into())), teinte.encre), teinte, quand)
        });
        let pied = montant.is_some() || echeance.is_some() || !c.responsables.is_empty();
        let hauteur = MARGE_DE_CARTE
            + titre.size().y
            + detail.as_ref().map_or(0.0, |d| d.size().y + 3.0)
            + if pied { 10.0 + 22.0 } else { 0.0 }
            + MARGE_DE_CARTE;
        CarteMiseEnPage { titre, detail, montant, echeance, hauteur }
    }
}

/// Les couloirs d'un tableau, côte à côte, de même hauteur (celle du plus chargé) : chacun sa tête et ses cartes. Rend
/// ce qu'on a choisi dans le menu d'une carte.
fn couloirs(ui: &mut egui::Ui, t: &Tableau, couloir: f32, aujourdhui: i64) -> Option<Action> {
    let ecart = 14.0;
    let p = ui.painter().clone();
    let largeur_de_carte = couloir - 16.0;
    let mises: Vec<Vec<CarteMiseEnPage>> = t
        .colonnes
        .iter()
        .map(|c| {
            let close = close(c);
            c.cartes.iter().map(|x| CarteMiseEnPage::nouvelle(&p, x, largeur_de_carte, aujourdhui, close)).collect()
        })
        .collect();
    let tete = 46.0;
    let hauteurs: Vec<f32> = t
        .colonnes
        .iter()
        .zip(&mises)
        .map(|(c, m)| {
            let cartes: f32 = m.iter().map(|x| x.hauteur + 8.0).sum();
            let pied = if pied_de_colonne(c).is_some() { 28.0 } else { 0.0 };
            tete + cartes + pied + 8.0
        })
        .collect();
    let hauteur = hauteurs.iter().copied().fold(160.0, f32::max);
    let n = t.colonnes.len() as f32;
    let (zone, _) = ui.allocate_exact_size(vec2(couloir * n + ecart * (n - 1.0), hauteur), Sense::hover());
    let mut action = None;
    for (j, (c, cartes)) in t.colonnes.iter().zip(mises).enumerate() {
        let r =
            Rect::from_min_size(pos2(zone.left() + (couloir + ecart) * j as f32, zone.top()), vec2(couloir, hauteur));
        p.add(theme::ombre_marine(r, 12, 1, 6, 10));
        p.rect(
            r,
            12.0,
            Color32::from_white_alpha(150),
            Stroke::new(1.0, Color32::from_white_alpha(230)),
            StrokeKind::Inside,
        );
        // La tête : le point de ce que dit la colonne, son titre, combien de cartes l'ERP y compte.
        let teinte = couleur_de_la_colonne(c);
        let y = r.top() + 23.0;
        p.circle_filled(pos2(r.left() + 18.0, y), 5.0, teinte.vif[1]);
        let compte = p.layout_no_wrap(
            centre::milliers(c.total.max(c.cartes.len()) as f64),
            FontId::new(12.0, FontFamily::Name(SEMI.into())),
            erp::TEXTE_2,
        );
        let pilule = Rect::from_min_size(
            pos2(r.right() - 14.0 - compte.size().x - 16.0, y - 11.0),
            vec2(compte.size().x + 16.0, 22.0),
        );
        let mut job = egui::text::LayoutJob::simple_singleline(
            en_francais(&c.titre),
            FontId::new(14.0, FontFamily::Name(SEMI.into())),
            erp::TITRE,
        );
        job.wrap = egui::text::TextWrapping::truncate_at_width((pilule.left() - r.left() - 40.0).max(30.0));
        let g = p.layout_job(job);
        p.galley(pos2(r.left() + 30.0, y - g.size().y / 2.0), g, erp::TITRE);
        p.rect_filled(pilule, 11.0, Color32::from_white_alpha(235));
        p.rect_stroke(pilule, 11.0, Stroke::new(1.0, erp::FILET_DE_CARTE), StrokeKind::Inside);
        p.galley(pilule.center() - compte.size() / 2.0, compte, erp::TEXTE_2);
        // Les cartes.
        let mut yc = r.top() + tete;
        let ailleurs: Vec<&Colonne> = t.colonnes.iter().enumerate().filter(|(i, _)| *i != j).map(|(_, x)| x).collect();
        let autres_colonnes: Vec<(char, String)> =
            ailleurs.iter().map(|x| ('\u{E72A}', en_francais(&x.titre))).collect();
        for (i, (carte, mise)) in c.cartes.iter().zip(cartes).enumerate() {
            let place = Rect::from_min_size(pos2(r.left() + 8.0, yc), vec2(largeur_de_carte, mise.hauteur));
            yc += mise.hauteur + 8.0;
            let cle = format!("carte-du-kanban-{}-{}-{i}", t.cle, carte.id);
            let rep = ui.interact(place, egui::Id::unique(cle.as_str()), Sense::click());
            let ouvert = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&rep));
            peindre_la_carte(&p, place, carte, mise, rep.hovered() || ouvert);
            let rep = rep.on_hover_cursor(egui::CursorIcon::PointingHand);
            let rep =
                if carte.responsables.is_empty() { rep } else { rep.on_hover_text(carte.responsables.join(", ")) };
            let choisie = menu_de_chose(&rep, &cle, &[], Some(("DÉPLACER VERS…", &autres_colonnes)));
            action = action.or(match choisie {
                Some(0) => Some(parler_de_la_carte(carte, &t.titre, &c.titre)),
                Some(1) => Some(rappel_de_la_carte(carte, &t.titre, &c.titre)),
                Some(k) => {
                    let vers = ailleurs[k - 2].titre.trim();
                    Some(deplacer_la_carte(carte, &t.titre, c.titre.trim(), vers, t.garde.as_deref()))
                }
                None => None,
            });
        }
        let pied = pied_de_colonne(c);
        if let Some(texte) = pied {
            let encre = if c.cartes.is_empty() { erp::TEXTE_3 } else { erp::TEXTE_2 };
            let police = FontId::proportional(12.0);
            p.text(pos2(r.left() + 14.0, yc + 10.0), Align2::LEFT_CENTER, texte, police, encre);
        }
    }
    action
}

/// Une carte dans `r` : blanche, son ombre marine (plus marquée au survol, elle se lève), son titre, son détail ; au
/// pied, son montant à gauche, son échéance dans une pastille de sa teinte, les initiales de ses responsables à droite.
fn peindre_la_carte(p: &egui::Painter, r: Rect, c: &Carte, m: CarteMiseEnPage, survol: bool) {
    if survol {
        p.add(theme::ombre_marine(r, 10, 6, 16, 40));
    } else {
        p.add(theme::ombre_marine(r, 10, 1, 4, 22));
    }
    p.rect(r, 10.0, Color32::WHITE, Stroke::new(1.0, erp::FILET_DE_CARTE), StrokeKind::Inside);
    let x = r.left() + MARGE_DE_CARTE;
    let h = m.titre.size().y;
    p.galley(pos2(x, r.top() + MARGE_DE_CARTE), m.titre, erp::TITRE);
    if let Some(d) = m.detail {
        p.galley(pos2(x, r.top() + MARGE_DE_CARTE + h + 3.0), d, erp::TEXTE_2);
    }
    let pied = m.montant.is_some() || m.echeance.is_some() || !c.responsables.is_empty();
    if !pied {
        return;
    }
    // Le pied, au bas de la carte.
    let ligne = pos2(x, r.bottom() - MARGE_DE_CARTE - 11.0);
    let mut gauche = ligne.x;
    if let Some(g) = m.montant {
        let l = g.size().x;
        p.galley(pos2(gauche, ligne.y - g.size().y / 2.0), g, erp::TITRE);
        gauche += l + 10.0;
    }
    if let Some((g, teinte, _)) = m.echeance {
        let police = FontId::new(11.0, FontFamily::Name(ICONES.into()));
        let taille = vec2(g.size().x + 30.0, 22.0);
        let pilule = Rect::from_min_size(pos2(gauche, ligne.y - 11.0), taille);
        p.rect_filled(pilule, 11.0, teinte.pastel.gamma_multiply(0.24));
        p.text(pos2(pilule.left() + 12.0, ligne.y), Align2::CENTER_CENTER, '\u{EC92}', police, teinte.encre);
        p.galley(pos2(pilule.left() + 22.0, ligne.y - g.size().y / 2.0), g, teinte.encre);
    }
    let mut droite = r.right() - MARGE_DE_CARTE;
    let noms: Vec<&String> = c.responsables.iter().filter(|n| !n.trim().is_empty()).collect();
    for nom in noms.iter().take(3).rev() {
        let c = pos2(droite - 11.0, ligne.y);
        p.circle_filled(c, 12.0, Color32::WHITE);
        p.circle_filled(c, 10.5, couleur_de_la_personne(nom));
        let police = FontId::new(9.5, FontFamily::Name(SEMI.into()));
        p.text(c, Align2::CENTER_CENTER, initiales(nom), police, Color32::WHITE);
        droite -= 17.0;
    }
}

#[cfg(test)]
mod essais {
    use super::*;

    fn colonne(statut: &str, titre: &str, total: usize, cartes: usize) -> Colonne {
        let carte = |k: usize| Carte {
            id: format!("c-{k}"),
            titre: format!("Carte {k}"),
            detail: String::new(),
            montant: None,
            echeance: None,
            responsables: Vec::new(),
            source: String::new(),
        };
        Colonne { statut: statut.into(), titre: titre.into(), total, cartes: (0..cartes).map(carte).collect() }
    }

    #[test]
    fn la_couleur_dit_l_etat_de_la_colonne() {
        assert_eq!(couleur_de_la_colonne(&colonne("ACCEPTE", "Acceptée", 0, 0)), erp::VERT);
        assert_eq!(couleur_de_la_colonne(&colonne("REFUSE", "Refusée", 0, 0)), erp::ROUGE);
        assert_eq!(couleur_de_la_colonne(&colonne("EN_ATTENTE", "En attente", 0, 0)), erp::AMBRE);
        assert_eq!(couleur_de_la_colonne(&colonne("ENVOYE", "Envoyée", 0, 0)), erp::BLEU);
        assert_eq!(couleur_de_la_colonne(&colonne("BROUILLON", "Brouillon", 0, 0)), erp::GRIS);
        assert_eq!(couleur_de_la_colonne(&colonne("X", "Inspection", 0, 0)), erp::INDIGO, "rien ne le dit");
    }

    #[test]
    fn une_colonne_dit_ce_qu_eve_n_a_pas_lu() {
        assert_eq!(
            pied_de_colonne(&colonne("ACCEPTE", "Acceptée", 14, 3)).as_deref(),
            Some("… et 11 autres dans Constructo AI")
        );
        let pied = |total, lues| pied_de_colonne(&colonne("ACCEPTE", "Acceptée", total, lues));
        assert_eq!(pied(4, 3).as_deref(), Some("… et 1 autre dans Constructo AI"));
        assert_eq!(pied(2, 3), None, "un total plus petit que ce qui est lu : rien");
        assert_eq!(pied(9, 0).as_deref(), Some("9 cartes dans Constructo AI"), "rien de lu, mais compté");
        assert_eq!(pied(1, 0).as_deref(), Some("1 carte dans Constructo AI"));
        assert_eq!(pied(0, 0).as_deref(), Some("Aucune carte"));
        assert_eq!(montant(1_450_000.0), "1,45\u{a0}M$");
        assert_eq!(montant(96_400.0), "96\u{202f}400\u{a0}$");
    }

    #[test]
    fn les_couloirs_partagent_la_place_ou_defilent() {
        assert_eq!(largeur_des_couloirs(4, 1200.0), (289.5, false));
        assert_eq!(largeur_des_couloirs(2, 1200.0), (320.0, false), "pas plus de 320 points");
        assert_eq!(largeur_des_couloirs(6, 900.0), (264.0, true), "trop étroits : ils défilent");
    }

    #[test]
    fn l_echeance_d_une_carte() {
        let aujourdhui = centre::numero_du_jour("2026-10-30").unwrap();
        let mut c = colonne("X", "X", 1, 1).cartes.remove(0);
        c.echeance = Some("2026-10-25".into());
        assert_eq!(echeance(&c, aujourdhui, false).map(|e| (e.0, e.2)), Some(("25 oct.".to_string(), "échue")));
        assert_eq!(echeance(&c, aujourdhui, true).map(|e| e.1), Some(erp::GRIS), "une colonne close : une date");
        c.echeance = Some("2026-11-04".into());
        assert_eq!(echeance(&c, aujourdhui, false).map(|e| e.1), Some(erp::AMBRE));
        c.echeance = Some("2026-12-04".into());
        assert_eq!(echeance(&c, aujourdhui, false).map(|e| e.1), Some(erp::GRIS));
        c.echeance = Some("bientôt".into());
        assert!(echeance(&c, aujourdhui, false).is_none());
        assert!(close(&colonne("TERMINE", "Termine", 0, 0)) && close(&colonne("REFUSE", "Refuse", 0, 0)));
        assert!(!close(&colonne("EN_COURS", "En cours", 0, 0)) && !close(&colonne("EN_ATTENTE", "En attente", 0, 0)));
    }
}
