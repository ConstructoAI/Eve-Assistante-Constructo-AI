//! Les profils dans la fenêtre native ([`crate::profils`]) : là où la barre montre le nom d'Eve, le profil actif, et,
//! s'il y en a plus d'un, son menu (l'actif coché) ; la rubrique « Profil » des réglages, la même liste. Un clic rend
//! le geste `profil` au chef, qui l'inscrit dans `reglages.json` et y passe entre deux demandes ; la fenêtre ne coche
//! que ce qu'il publie. La barre et les réglages servent les deux thèmes : ce module aussi.

use std::path::PathBuf;

use eframe::egui::{self, Align2, Color32, FontFamily, FontId, Margin, Sense, Stroke, pos2, vec2};
use serde_json::Value;

use super::{
    Fenetre, ICONES, PANNEAU_CLAIR, PANNEAU_SOMBRE, Panneau, SEMI, couleurs, groupe, icones, panneau, rubrique,
};

/// Le chevron du menu (ChevronDown, de Segoe Fluent Icons).
const CHEVRON: char = '\u{E70D}';

/// Le nom sous « EVE », dans la barre, quand il ouvre le menu des profils : pour l'essai qui le clique.
pub(crate) fn id_du_menu_des_profils() -> egui::Id {
    egui::Id::unique("menu-des-profils")
}

/// Un profil du menu de la barre, par son nom : pour l'essai qui le clique.
pub(crate) fn id_du_profil(nom: &str) -> egui::Id {
    egui::Id::unique(("profil-du-menu", nom))
}

/// Un profil de la rubrique des réglages, par son nom.
pub(crate) fn id_du_profil_des_reglages(nom: &str) -> egui::Id {
    egui::Id::unique(("profil-des-reglages", nom))
}

/// Un profil tel que le publie le chef : son nom, son dossier, et s'il est celui d'office.
struct Publie {
    nom: String,
    dossier: String,
    office: bool,
}

impl Fenetre {
    /// Les profils que publie le chef, celui d'office d'abord ; aucun pour un état qui n'en dit rien.
    fn profils(&self) -> Vec<Publie> {
        let texte = |p: &Value, cle: &str| p.get(cle).and_then(Value::as_str).unwrap_or("").to_string();
        self.etat
            .get("profils")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|p| Publie {
                nom: texte(p, "nom"),
                dossier: texte(p, "dossier"),
                office: p.get("office").and_then(Value::as_bool).unwrap_or(false),
            })
            .filter(|p| !p.nom.is_empty())
            .collect()
    }

    /// Le nom du profil actif (sans profils publiés : le nom d'Eve).
    fn profil_actif(&self) -> String {
        let p = self.champ("profil");
        if p.is_empty() { self.champ("nom") } else { p }.to_string()
    }

    /// Le nom sous « EVE », dans la barre : celui du profil actif ; avec plus d'un profil, il ouvre leur menu. Ce qui
    /// s'y clique est noté pour Windows ([`super::cadre`]) ; le menu, une couche au-dessus, l'est de lui-même.
    pub(super) fn nom_et_profils(&mut self, ui: &mut egui::Ui) {
        let actif = self.profil_actif();
        let profils = self.profils();
        if profils.len() < 2 {
            ui.label(super::semi(actif, 12.0).color(couleurs::BARRE_TEXTE));
            return;
        }
        let police = FontId::new(12.0, FontFamily::Name(SEMI.into()));
        let nom = ui.painter().layout_no_wrap(actif.clone(), police, couleurs::BARRE_TEXTE);
        let chevron = ui.painter().layout_no_wrap(
            CHEVRON.to_string(),
            FontId::new(9.0, FontFamily::Name(ICONES.into())),
            couleurs::BARRE_TEXTE,
        );
        // Serré sous « EVE de Constructo AI », et aligné sur lui : le nom au bord de sa place ; le fond du survol déborde
        // autour, sans rien pousser.
        let (place, _) = ui.allocate_exact_size(vec2(nom.size().x + 6.0 + chevron.size().x, 16.0), Sense::hover());
        let rect = place.expand2(vec2(5.0, 2.0));
        let rep = ui.interact(rect, id_du_menu_des_profils(), Sense::click());
        self.zones_de_la_barre.push(rect);
        let ouvert = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&rep));
        let p = ui.painter();
        if rep.hovered() || ouvert {
            p.rect_filled(rect, 5.0, Color32::from_white_alpha(if ouvert { 30 } else { 20 }));
        }
        let x = place.left();
        p.galley(pos2(x, rect.center().y - nom.size().y / 2.0), nom.clone(), couleurs::BARRE_TEXTE);
        let x = x + nom.size().x + 6.0;
        p.galley(pos2(x, rect.center().y - chevron.size().y / 2.0), chevron, couleurs::BARRE_TEXTE);
        let rep = rep.on_hover_cursor(egui::CursorIcon::PointingHand);
        // La largeur du menu, celle de son plus long nom : ses lignes la prennent toutes (la place libre d'une couche
        // au-dessus serait celle de l'écran).
        let mesure =
            |texte: &str, police: FontId| ui.painter().layout_no_wrap(texte.to_string(), police, Color32::WHITE);
        let d_office = mesure("d'office", FontId::proportional(11.5)).size().x + 12.0;
        let largeur = profils
            .iter()
            .map(|p| 30.0 + mesure(&p.nom, police_du_menu()).size().x + if p.office { d_office } else { 0.0 } + 14.0)
            .fold(220.0, f32::max);
        let mut choisi = None;
        // Le menu, sur sa propre couche, prend les visuels de son thème : dans le clair, ceux de ses panneaux.
        let clair = self.theme_constructo(ui.ctx());
        let pan = if clair { &PANNEAU_CLAIR } else { &PANNEAU_SOMBRE };
        let style = move |s: &mut egui::Style| {
            if clair {
                super::theme::visuels_du_panneau(&mut s.visuals);
            }
            egui::containers::menu::menu_style(s);
        };
        egui::Popup::menu(&rep).gap(6.0).frame(cadre_du_menu(pan)).style(style).show(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            for p in &profils {
                if ligne_du_menu(ui, p, crate::profils::meme(&p.nom, &actif), largeur).clicked() {
                    choisi = Some(p.nom.clone());
                }
            }
        });
        if let Some(nom) = choisi.filter(|n| !crate::profils::meme(n, &actif)) {
            self.geste("profil", Value::String(nom));
        }
    }

    /// La rubrique « Profil » des réglages : chaque profil, l'actif coché ; un clic le choisit. Même seul, celui
    /// d'office s'y montre, avec ce qu'est un profil : la personne sait qu'Eve peut lui en créer.
    pub(super) fn rubrique_des_profils(&mut self, ui: &mut egui::Ui) {
        let profils = self.profils();
        if profils.is_empty() {
            return;
        }
        let actif = self.profil_actif();
        let mut choisi = None;
        rubrique(ui, "Profil");
        groupe(ui, |ui| {
            for (i, p) in profils.iter().enumerate() {
                let est_actif = crate::profils::meme(&p.nom, &actif);
                if rangee_de_profil(ui, p, est_actif, i == 0).clicked() && !est_actif {
                    choisi = Some(p.nom.clone());
                }
            }
        });
        ui.add_space(6.0);
        let aide = "Un profil : un dossier avec son CLAUDE.md, et sa conversation. Demande à Eve d'en créer un ; elle \
                    change de profil entre deux demandes.";
        ui.label(egui::RichText::new(aide).size(12.0).color(panneau(ui).encre_3));
        if let Some(nom) = choisi {
            self.geste("profil", Value::String(nom));
        }
    }

    /// Les dossiers des profils publiés : les aperçus des cartes s'y lisent aussi
    /// ([`crate::cartes::Cartes::permettre`]), comme dans celui d'Eve — un plan que Claude dessine dans le dossier où
    /// il travaille s'affiche.
    pub(super) fn suivre_les_dossiers_des_profils(&mut self) {
        let dossiers: Vec<PathBuf> =
            self.profils().into_iter().filter(|p| !p.dossier.is_empty()).map(|p| PathBuf::from(p.dossier)).collect();
        if !dossiers.is_empty() && dossiers != self.dossiers_permis {
            self.cartes.permettre(dossiers.clone());
            self.dossiers_permis = dossiers;
        }
    }
}

/// Le cadre du menu des profils, celui des panneaux de la fenêtre : leur fond, leur filet, des coins arrondis, leur
/// ombre.
fn cadre_du_menu(p: &Panneau) -> egui::Frame {
    egui::Frame::new()
        .fill(p.fond)
        .stroke((1.0, p.filet))
        .corner_radius(10)
        .inner_margin(Margin::same(6))
        .shadow(p.ombres[1])
}

/// Le nom d'un profil dans le menu.
fn police_du_menu() -> FontId {
    FontId::new(13.0, FontFamily::Name(SEMI.into()))
}

/// Une ligne du menu, de toute sa largeur : la coche de l'actif, le nom, « d'office » pour celui d'Eve ; au survol, un
/// fond, et son dossier.
fn ligne_du_menu(ui: &mut egui::Ui, p: &Publie, actif: bool, largeur: f32) -> egui::Response {
    let (rect, _) = ui.allocate_exact_size(vec2(largeur, 30.0), Sense::hover());
    let rep = ui.interact(rect, id_du_profil(&p.nom), Sense::click());
    let pan = panneau(ui);
    let painter = ui.painter();
    if rep.hovered() {
        painter.rect_filled(rect, 6.0, pan.survol);
    }
    if actif {
        let coche = FontId::new(12.0, FontFamily::Name(ICONES.into()));
        painter.text(pos2(rect.left() + 14.0, rect.center().y), Align2::CENTER_CENTER, icones::OUI, coche, pan.accent);
    }
    // L'actif se détache : le plus clair des textes dans le sombre, le plus foncé dans le clair.
    let encre = if actif && ui.visuals().dark_mode { Color32::WHITE } else { pan.encre };
    let nom =
        painter.text(pos2(rect.left() + 30.0, rect.center().y), Align2::LEFT_CENTER, &p.nom, police_du_menu(), encre);
    if p.office {
        let x = (rect.right() - 10.0).max(nom.right() + 12.0);
        let petit = FontId::proportional(11.5);
        painter.text(pos2(x, rect.center().y), Align2::RIGHT_CENTER, "d'office", petit, pan.encre_3);
    }
    let rep = rep.on_hover_cursor(egui::CursorIcon::PointingHand);
    if p.dossier.is_empty() { rep } else { rep.on_hover_text(&p.dossier) }
}

/// Une rangée de la rubrique des réglages (comme celle d'un interrupteur) : le nom, et sous lui son dossier ; à
/// droite, la pastille de l'actif (pleine, cochée) ou le rond d'un autre ; un filet au-dessus, sauf la première.
fn rangee_de_profil(ui: &mut egui::Ui, p: &Publie, actif: bool, premier: bool) -> egui::Response {
    let largeur = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(largeur, 46.0), Sense::hover());
    let rep = ui.interact(rect, id_du_profil_des_reglages(&p.nom), Sense::click());
    let pan = panneau(ui);
    let painter = ui.painter();
    if !premier {
        painter.hline(rect.x_range(), rect.top() + 0.5, Stroke::new(1.0, pan.filet));
    }
    let nom = if p.office { format!("{} (d'office)", p.nom) } else { p.nom.clone() };
    let police = FontId::proportional(14.0);
    painter.text(pos2(rect.left(), rect.center().y - 8.0), Align2::LEFT_CENTER, nom, police, pan.encre);
    let dossier = chemin_court(ui, &p.dossier, largeur - 40.0);
    painter.text(pos2(rect.left(), rect.center().y + 9.0), Align2::LEFT_CENTER, dossier, petite(), pan.encre_3);
    let centre = pos2(rect.right() - 12.0, rect.center().y);
    if actif {
        painter.circle_filled(centre, 10.0, pan.accent);
        let coche = FontId::new(10.0, FontFamily::Name(ICONES.into()));
        painter.text(centre, Align2::CENTER_CENTER, icones::OUI, coche, pan.sur_plein);
    } else {
        let anneau = if rep.hovered() { pan.encre_2 } else { pan.filet_fort };
        painter.circle_stroke(centre, 9.0, Stroke::new(1.5, anneau));
    }
    let rep = if actif { rep } else { rep.on_hover_cursor(egui::CursorIcon::PointingHand) };
    if p.dossier.is_empty() { rep } else { rep.on_hover_text(&p.dossier) }
}

/// Le dossier sous le nom d'un profil, en petit.
fn petite() -> FontId {
    FontId::proportional(11.5)
}

/// Un chemin sur une ligne de `largeur` points : entier s'il y tient, sinon ses deux derniers dossiers après « … »,
/// sinon le dernier — la fin d'un chemin dit de quel dossier il s'agit (son survol le donne entier).
fn chemin_court(ui: &egui::Ui, chemin: &str, largeur: f32) -> String {
    let tient = |t: &str| ui.painter().layout_no_wrap(t.to_string(), petite(), Color32::WHITE).size().x <= largeur;
    let morceaux: Vec<&str> = chemin.split(['\\', '/']).filter(|m| !m.is_empty()).collect();
    let fin = |n: usize| format!("…\\{}", morceaux[morceaux.len().saturating_sub(n)..].join("\\"));
    if tient(chemin) {
        chemin.to_string()
    } else if tient(&fin(2)) {
        fin(2)
    } else {
        fin(1)
    }
}
