//! L'historique de l'onglet « Comptabilité » : le journal d'audit de la comptabilité de l'ERP, mis en mots par
//! `crate::compta::historique`, dans une carte du thème — une recherche en texte, puis un changement par rangée (quand,
//! qui, ce qui s'est passé, sur quoi, le détail), le plus récent en haut, l'en-tête dans son bandeau marine comme les
//! sommaires. Un clic sur une rangée ouvre son menu : « En parler à Eve » (la facture qu'elle touche, nommée), « Me le
//! rappeler demain » — des demandes à Eve, jamais une écriture dans l'ERP.
//! L'heure : l'instant du serveur (en UTC) passe à l'heure du poste par Windows (`SystemTimeToTzSpecificLocalTime`),
//! qui sait l'heure d'été de CE jour-là — une entrée d'avant le 1er novembre se dit à l'heure avancée, même lue après.
//! Le décalage du poste à l'instant même (`rappels::heure_locale`) se tromperait d'une heure sur tout ce qui précède un
//! changement d'heure.

use std::hash::{Hash, Hasher};
use std::sync::Arc;

use eframe::egui::{self, Align2, Color32, FontFamily, FontId, Margin, Rect, Sense, Stroke, StrokeKind, pos2, vec2};
use serde_json::Value;

use super::super::Fenetre;
use super::super::planning::etat_vide;
use super::super::theme::{self, erp};
use super::super::{ICONES, SEMI, insecables};
use super::Contexte;
use super::sommaires::{Colonne, JEU, candidats, largeur_d_entete, peindre_l_entete, plus_large};
use crate::compta::historique::{self as calcul, Entree};

/// L'icône de l'historique (History, Segoe Fluent Icons), et la loupe de la recherche (Search).
const ICONE: char = '\u{E81C}';
const LOUPE: char = '\u{E721}';
/// Les hauteurs d'une rangée : sur une ligne, et sur deux (fenêtre étroite).
const HAUTEUR: f32 = 34.0;
const HAUTEUR_ETROITE: f32 = 52.0;
const MARGE: f32 = 10.0;

// ---- L'heure du poste -----------------------------------------------------------------------------------------------

/// L'heure locale du poste d'un instant Unix (ses secondes civiles), l'heure d'été de ce jour-là comprise : Windows la
/// sait d'après le fuseau du poste ; sinon (ailleurs que sous Windows, ou une date qu'il refuse), le décalage du poste
/// à l'instant même.
fn en_heure_du_poste(unix: i64) -> Option<i64> {
    par_windows(unix).or_else(|| Some(unix + decalage_du_moment()))
}

/// Le décalage du poste à l'instant même (l'heure locale, moins l'heure Unix), au quart d'heure, comme
/// `entreprise::maintenant_iso`.
fn decalage_du_moment() -> i64 {
    let unix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
    crate::rappels::en_secondes(&crate::journal::date_et_heure())
        .map_or(0, |l| ((l - unix) as f64 / 900.0).round() as i64 * 900)
}

#[cfg(windows)]
fn par_windows(unix: i64) -> Option<i64> {
    use windows::Win32::Foundation::SYSTEMTIME;
    // Dans kernel32, que tout programme de Windows charge ; la fonctionnalité `Win32_System_Time` du paquet `windows`,
    // qui la déclare, n'est pas prise (elle recompilerait le paquet pour une seule fonction).
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn SystemTimeToTzSpecificLocalTime(
            fuseau: *const std::ffi::c_void,
            universelle: *const SYSTEMTIME,
            locale: *mut SYSTEMTIME,
        ) -> i32;
    }
    let date = crate::centre::date_du_numero(unix.div_euclid(86_400));
    let s = unix.rem_euclid(86_400);
    let champ = |a: usize, b: usize| date.get(a..b).and_then(|x| x.parse::<u16>().ok());
    let universelle = SYSTEMTIME {
        wYear: champ(0, 4)?,
        wMonth: champ(5, 7)?,
        wDayOfWeek: 0,
        wDay: champ(8, 10)?,
        wHour: (s / 3600) as u16,
        wMinute: (s / 60 % 60) as u16,
        wSecond: (s % 60) as u16,
        wMilliseconds: 0,
    };
    let mut locale = SYSTEMTIME::default();
    // SAFETY : les deux structures vivent le temps de l'appel ; un fuseau nul demande celui du poste ; Windows écrit
    // dans `locale` seulement.
    if unsafe { SystemTimeToTzSpecificLocalTime(std::ptr::null(), &raw const universelle, &raw mut locale) } == 0 {
        return None;
    }
    let jour = crate::centre::numero_du_jour(&format!("{:04}-{:02}-{:02}", locale.wYear, locale.wMonth, locale.wDay))?;
    Some(jour * 86_400 + i64::from(locale.wHour) * 3600 + i64::from(locale.wMinute) * 60 + i64::from(locale.wSecond))
}

#[cfg(not(windows))]
fn par_windows(_unix: i64) -> Option<i64> {
    None
}

// ---- La mémoire -----------------------------------------------------------------------------------------------------

/// Les entrées, gardées dans la mémoire d'egui avec l'empreinte de ce qui les a faites (la lecture, son nombre de
/// changements, le jour) : elles ne se refont que si elle change.
#[derive(Clone)]
struct EntreesEnMemoire(u64, Arc<Vec<Entree>>);

fn entrees(ctx: &egui::Context, c: &Contexte<'_>) -> Arc<Vec<Entree>> {
    let id = egui::Id::unique("compta-historique-entrees");
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (c.lecture.lu_le.as_str(), c.registre.historique.len(), c.registre.factures.len(), c.aujourdhui).hash(&mut h);
    let cle = h.finish();
    if let Some(EntreesEnMemoire(k, e)) = ctx.data(|d| d.get_temp::<EntreesEnMemoire>(id))
        && k == cle
    {
        return e;
    }
    let e = Arc::new(calcul::entrees(c.registre, c.aujourdhui, &en_heure_du_poste));
    ctx.data_mut(|d| d.insert_temp(id, EntreesEnMemoire(cle, e.clone())));
    e
}

// ---- La page --------------------------------------------------------------------------------------------------------

/// Ce qu'on a choisi dans le menu d'une rangée.
enum Choisi {
    Parler(usize),
    Rappeler(usize),
}

pub(super) fn page(f: &mut Fenetre, ui: &mut egui::Ui, c: &Contexte<'_>) {
    let r = c.registre;
    if r.historique_absent {
        let texte = "Le journal de la comptabilité n'existe pas encore dans Constructo AI : il commence à la première \
                     opération notée.";
        etat_vide(ui, ICONE, erp::INDIGO, "L'historique de la comptabilité", texte, None);
        return;
    }
    if r.historique.is_empty() {
        let (titre, texte, panne) = sans_entree(r);
        etat_vide(ui, ICONE, if panne { erp::AMBRE } else { erp::INDIGO }, titre, &texte, None);
        return;
    }
    let toutes = entrees(ui.ctx(), c);
    let id_recherche = egui::Id::unique("compta-historique-recherche");
    let mut recherche: String = ui.ctx().data(|d| d.get_temp(id_recherche)).unwrap_or_default();
    let mut choisi = None;
    theme::carte(ui, Some(erp::INDIGO.pastel), 0.0, |ui| {
        let n = toutes.len();
        let badge = format!("{}\u{a0}changement{}", crate::centre::milliers(n as f64), if n > 1 { "s" } else { "" });
        theme::tete_de_carte(ui, "Historique", ICONE, erp::INDIGO, false, Some((&badge, erp::INDIGO)));
        let sous = "Le journal de la comptabilité de Constructo AI : qui a fait quoi, et quand — le plus récent en \
                    haut. Le journal entier : la période et le chantier d'en tête n'y touchent pas.";
        let sous = egui::RichText::new(insecables(sous)).size(13.0).color(erp::TEXTE_2);
        ui.add(egui::Label::new(sous).wrap().selectable(false));
        if let Some(texte) = ce_qui_manque(r, n) {
            let texte = egui::RichText::new(insecables(&texte)).size(13.0).color(erp::AVERTISSEMENT_ENCRE);
            ui.add(egui::Label::new(texte).wrap().selectable(false));
        }
        ui.add_space(6.0);
        if champ_de_recherche(ui, &mut recherche) {
            ui.ctx().data_mut(|d| d.insert_temp(id_recherche, recherche.clone()));
        }
        let retenues = calcul::retenues(&toutes, &recherche);
        if !recherche.trim().is_empty() {
            let texte = match retenues.len() {
                0 => "Aucun changement ne répond à cette recherche.".to_string(),
                1 => format!("1 changement sur {}", crate::compta::par_milliers(n as u64)),
                k => format!(
                    "{} changements sur {}",
                    crate::compta::par_milliers(k as u64),
                    crate::compta::par_milliers(n as u64)
                ),
            };
            ui.label(egui::RichText::new(texte).size(12.5).color(erp::TEXTE_2));
        }
        ui.add_space(4.0);
        if !retenues.is_empty() {
            choisi = if c.etroite { en_rangees(ui, &toutes, &retenues) } else { en_tableau(ui, &toutes, &retenues) };
        }
    });
    let demande = |k: usize, parler: bool| {
        let e = &toutes[k];
        let facture = e.facture.and_then(|i| r.factures.get(i));
        if parler { calcul::demande(e, facture) } else { calcul::rappel(e, facture) }
    };
    match choisi {
        Some(Choisi::Parler(k)) => f.geste("texte", Value::String(demande(k, true))),
        Some(Choisi::Rappeler(k)) => f.geste("rappel_demain", Value::String(demande(k, false))),
        None => {}
    }
}

/// L'historique sans entrée à montrer : le titre et le texte de son état vide, et s'il dit une panne — l'historique
/// arrêté par une panne du serveur (`historique_erreur` : la lecture a gardé le registre) la dit, au lieu d'un journal
/// « vide » qui ne l'est pas ; sinon, le journal vide.
fn sans_entree(r: &crate::compta::Registre) -> (&'static str, String, bool) {
    match r.historique_erreur.trim() {
        "" => (
            "L'historique de la comptabilité",
            "Aucun changement dans le journal de la comptabilité de Constructo AI pour le moment.".into(),
            false,
        ),
        erreur => ("L'historique ne s'est pas lu", erreur.to_string(), true),
    }
}

/// Ce que la carte dit de ce qui manque à l'historique (`n` : ses entrées lues) : la panne qui l'a arrêté, s'il y en a
/// une ; sinon sa borne, s'il a été tronqué ; rien s'il s'est lu en entier.
fn ce_qui_manque(r: &crate::compta::Registre, n: usize) -> Option<String> {
    let lues = match n {
        1 => "le changement le plus récent".to_string(),
        n => format!("les {} changements les plus récents", crate::centre::milliers(n as f64)),
    };
    if !r.historique_erreur.trim().is_empty() {
        return Some(format!("{} Eve en a lu {lues}.", r.historique_erreur.trim()));
    }
    r.historique_tronque
        .then(|| format!("Eve a lu {lues} du journal : les plus anciens ne sont pas ici. Demande-les à Eve au besoin."))
}

/// Le champ de la recherche, comme celui de l'historique des conversations : la loupe, le texte, × pour l'effacer ;
/// cerné de l'accent quand on y écrit. Rend vrai si le texte a changé.
fn champ_de_recherche(ui: &mut egui::Ui, texte: &mut String) -> bool {
    let largeur = ui.available_width().min(520.0);
    let (rect, _) = ui.allocate_exact_size(vec2(largeur, 36.0), Sense::hover());
    let id = egui::Id::unique("compta-historique-champ");
    let actif = ui.ctx().memory(|m| m.has_focus(id));
    let p = ui.painter();
    if actif {
        p.rect_filled(rect.expand(3.0), 12.0, erp::PRIMAIRE.gamma_multiply(0.16));
    }
    let bord = if actif { erp::PRIMAIRE } else { erp::FILET_BOUTON };
    p.rect(rect, 9.0, Color32::WHITE, Stroke::new(1.0, bord), StrokeKind::Inside);
    let icones = FontFamily::Name(ICONES.into());
    p.text(
        pos2(rect.left() + 13.0, rect.center().y),
        Align2::LEFT_CENTER,
        LOUPE,
        FontId::new(13.0, icones.clone()),
        erp::TEXTE_3,
    );
    let effacer = !texte.is_empty();
    let droite = rect.right() - if effacer { 34.0 } else { 10.0 };
    let zone = Rect::from_min_max(pos2(rect.left() + 36.0, rect.top()), pos2(droite, rect.bottom()));
    let champ = ui.put(
        zone,
        egui::TextEdit::singleline(texte)
            .id(id)
            .frame(egui::Frame::new())
            .margin(Margin::ZERO)
            .align(Align2::LEFT_CENTER)
            .font(FontId::proportional(14.0))
            .text_color(erp::TEXTE)
            .hint_text(egui::RichText::new("Chercher : une personne, une facture, un mot…").color(erp::TEXTE_3))
            .desired_width(zone.width()),
    );
    let mut change = champ.changed();
    if effacer {
        let r = Rect::from_center_size(pos2(rect.right() - 18.0, rect.center().y), vec2(24.0, 24.0));
        let rep = ui.interact(r, egui::Id::unique("compta-historique-effacer"), Sense::click());
        if rep.hovered() {
            ui.painter().circle_filled(r.center(), 11.0, erp::SURVOL);
        }
        let encre = if rep.hovered() { erp::TEXTE } else { erp::TEXTE_3 };
        ui.painter().text(r.center(), Align2::CENTER_CENTER, '\u{E711}', FontId::new(10.0, icones), encre);
        if rep.on_hover_text("Effacer la recherche").on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            texte.clear();
            change = true;
        }
    }
    change
}

/// Les colonnes de l'historique, et leur texte le plus long d'office (les colonnes larges se partagent le reste).
fn colonnes() -> Vec<Colonne> {
    ["Quand", "Qui", "Ce qui s'est passé", "Sur quoi", "Détail"]
        .iter()
        .map(|t| Colonne { groupe: "", titre: (*t).to_string(), droite: false })
        .collect()
}

/// Le menu d'une rangée : « En parler à Eve », « Me le rappeler demain ».
fn menu(rep: &egui::Response, k: usize) -> Option<Choisi> {
    let mut choisi = None;
    theme::menu_blanc(rep).show(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        let id = egui::Id::unique(("compta-historique-menu", k));
        if theme::ligne_du_menu(ui, theme::pictos::PARLER, "En parler à Eve", false, id.with(0)).clicked() {
            choisi = Some(Choisi::Parler(k));
        }
        if theme::ligne_du_menu(ui, theme::pictos::RAPPELS, "Me le rappeler demain", false, id.with(1)).clicked() {
            choisi = Some(Choisi::Rappeler(k));
        }
    });
    choisi
}

/// La bulle d'une rangée : tout ce qu'elle dit, sans ellipse.
fn bulle(e: &Entree) -> String {
    let detail = if e.detail.is_empty() { String::new() } else { format!("\n{}", e.detail) };
    format!("{} — {}\n{}, {}{detail}", e.quoi, e.sur, e.quand, e.par)
}

/// Un texte sur une ligne, coupé d'une ellipse s'il dépasse `largeur`.
fn sur_une_ligne(p: &egui::Painter, texte: &str, police: FontId, couleur: Color32, largeur: f32) -> Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(texte.to_string(), police, couleur);
    job.wrap = egui::text::TextWrapping::truncate_at_width(largeur.max(10.0));
    p.layout_job(job)
}

/// Les changements en tableau : l'en-tête (qui reste en vue quand la liste défile), puis les rangées à l'écran
/// seulement. Rend ce qu'on a choisi dans le menu d'une rangée.
fn en_tableau(ui: &mut egui::Ui, toutes: &[Entree], retenues: &[usize]) -> Option<Choisi> {
    let largeur = ui.available_width();
    let p = ui.painter().clone();
    let cols = colonnes();
    // Quand, qui et ce qui s'est passé à leur largeur naturelle (bornée), leur titre compris ; sur quoi et le détail se
    // partagent le reste. Mesurée sur les textes qui peuvent être les plus larges (les 400 premières rangées).
    let largeur_de = |k: usize, f: &dyn Fn(&Entree) -> &str, police: FontId, max: f32| {
        let textes = candidats(retenues.iter().take(400).map(|i| f(&toutes[*i])));
        let corps = plus_large(&p, &textes, police);
        corps.max(largeur_d_entete(&p, &cols[k].titre)).min(max) + 2.0 * MARGE + JEU
    };
    let l_quand = largeur_de(0, &|e| e.quand.as_str(), FontId::proportional(13.0), 190.0);
    let l_qui = largeur_de(1, &|e| e.qui.as_str(), FontId::proportional(13.0), 230.0);
    let l_quoi = largeur_de(2, &|e| e.quoi.as_str(), FontId::new(13.0, FontFamily::Name(SEMI.into())), 260.0);
    // Sur quoi et le détail sans la place de se lire : les rangées de deux lignes.
    let reste = largeur - l_quand - l_qui - l_quoi;
    if reste < 280.0 {
        return en_rangees(ui, toutes, retenues);
    }
    let places = vec![
        (0.0, l_quand),
        (l_quand, l_qui),
        (l_quand + l_qui, l_quoi),
        (l_quand + l_qui + l_quoi, reste * 0.42),
        (l_quand + l_qui + l_quoi + reste * 0.42, reste * 0.58),
    ];
    let h_entete = 30.0;
    let n = retenues.len();
    let (zone, _) = ui.allocate_exact_size(vec2(largeur, h_entete + n as f32 * HAUTEUR), Sense::hover());
    let corps_haut = zone.top() + h_entete;
    let ecran = ui.clip_rect();
    // L'en-tête : à sa place, ou contre le haut de l'écran tant que la liste continue dessous ; une rangée qu'il couvre
    // ne répond pas à la souris (le clic irait à une rangée cachée).
    let y_entete = zone.top().max(ecran.top()).min(zone.bottom() - h_entete - HAUTEUR).max(zone.top());
    let couvert = y_entete + h_entete;
    let premiere = (((ecran.top() - corps_haut) / HAUTEUR).floor().max(0.0) as usize).min(n);
    let derniere = (((ecran.bottom() - corps_haut) / HAUTEUR).ceil().max(0.0) as usize).min(n);
    let mut choisi = None;
    for j in premiere..derniere {
        let k = retenues[j];
        let e = &toutes[k];
        let r = Rect::from_min_size(pos2(zone.left(), corps_haut + j as f32 * HAUTEUR), vec2(largeur, HAUTEUR));
        let visible = Rect::from_min_max(pos2(r.left(), r.top().max(couvert)), r.max);
        let sens = if visible.height() >= HAUTEUR / 2.0 { Sense::click() } else { Sense::hover() };
        let rep = ui.interact(visible, egui::Id::unique(("compta-historique-rangee", e.id, k)), sens);
        let ouvert = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&rep));
        if rep.hovered() || ouvert {
            p.rect_filled(r.shrink2(vec2(0.0, 1.0)), 6.0, erp::SURVOL);
        }
        let textes: [(&str, FontId, Color32); 5] = [
            (&e.quand, FontId::proportional(13.0), erp::TEXTE_2),
            (&e.qui, FontId::proportional(13.0), erp::TEXTE),
            (&e.quoi, FontId::new(13.0, FontFamily::Name(SEMI.into())), erp::TITRE),
            (&e.sur, FontId::proportional(13.0), if e.facture.is_some() { erp::PRIMAIRE } else { erp::TEXTE }),
            (&e.detail, FontId::proportional(13.0), erp::TEXTE_2),
        ];
        for ((texte, police, couleur), (x, w)) in textes.into_iter().zip(&places) {
            let g = sur_une_ligne(&p, texte, police, couleur, w - 2.0 * MARGE);
            p.galley(pos2(zone.left() + x + MARGE, r.center().y - g.size().y / 2.0), g, couleur);
        }
        if j + 1 < n {
            p.hline(r.x_range(), r.bottom() - 0.5, Stroke::new(1.0, erp::FILET_DE_CARTE));
        }
        let rep = rep.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(bulle(e));
        if let Some(c) = menu(&rep, k) {
            choisi = Some(c);
        }
    }
    peindre_l_entete(&p, zone.left(), y_entete, largeur, &cols, &places);
    choisi
}

/// Les changements en rangées de deux lignes (une fenêtre étroite) : ce qui s'est passé et sur quoi, puis quand, qui
/// et le détail.
fn en_rangees(ui: &mut egui::Ui, toutes: &[Entree], retenues: &[usize]) -> Option<Choisi> {
    let largeur = ui.available_width();
    let p = ui.painter().clone();
    let n = retenues.len();
    let (zone, _) = ui.allocate_exact_size(vec2(largeur, n as f32 * HAUTEUR_ETROITE), Sense::hover());
    let ecran = ui.clip_rect();
    let premiere = (((ecran.top() - zone.top()) / HAUTEUR_ETROITE).floor().max(0.0) as usize).min(n);
    let derniere = (((ecran.bottom() - zone.top()) / HAUTEUR_ETROITE).ceil().max(0.0) as usize).min(n);
    let mut choisi = None;
    for j in premiere..derniere {
        let k = retenues[j];
        let e = &toutes[k];
        let r = Rect::from_min_size(
            pos2(zone.left(), zone.top() + j as f32 * HAUTEUR_ETROITE),
            vec2(largeur, HAUTEUR_ETROITE),
        );
        let rep = ui.interact(r, egui::Id::unique(("compta-historique-rangee-etroite", e.id, k)), Sense::click());
        let ouvert = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&rep));
        if rep.hovered() || ouvert {
            p.rect_filled(r.shrink2(vec2(0.0, 1.0)), 6.0, erp::SURVOL);
        }
        let mut job = egui::text::LayoutJob::default();
        let fort = egui::TextFormat {
            font_id: FontId::new(13.5, FontFamily::Name(SEMI.into())),
            color: erp::TITRE,
            ..Default::default()
        };
        job.append(&e.quoi, 0.0, fort);
        let sur = egui::TextFormat {
            font_id: FontId::proportional(13.5),
            color: if e.facture.is_some() { erp::PRIMAIRE } else { erp::TEXTE },
            ..Default::default()
        };
        job.append(&format!(" \u{2014} {}", e.sur), 0.0, sur);
        job.wrap = egui::text::TextWrapping::truncate_at_width(largeur - 2.0 * MARGE);
        p.galley(pos2(r.left() + MARGE, r.top() + 7.0), p.layout_job(job), erp::TITRE);
        let morceaux: Vec<&str> =
            [e.quand.as_str(), e.qui.as_str(), e.detail.as_str()].into_iter().filter(|m| !m.is_empty()).collect();
        let g =
            sur_une_ligne(&p, &morceaux.join(" · "), FontId::proportional(12.5), erp::TEXTE_2, largeur - 2.0 * MARGE);
        p.galley(pos2(r.left() + MARGE, r.top() + 29.0), g, erp::TEXTE_2);
        if j + 1 < n {
            p.hline(r.x_range(), r.bottom() - 0.5, Stroke::new(1.0, erp::FILET_DE_CARTE));
        }
        let rep = rep.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(bulle(e));
        if let Some(c) = menu(&rep, k) {
            choisi = Some(c);
        }
    }
    choisi
}

#[cfg(all(test, windows))]
mod essais {
    use super::*;

    /// L'historique arrêté par une panne la dit — dans l'état vide s'il n'a rien lu, dans la carte sinon — au lieu d'un
    /// journal vide ou seulement « tronqué » (la QA5 du lot 13.2 : la panne de l'historique jetait tout le registre).
    #[test]
    fn la_panne_de_l_historique_se_dit() {
        use crate::compta::Registre;
        let erreur = "Constructo AI n'a pas pu lire l'historique : « Erreur lors de la lecture (voir logs serveur) ».";
        let vide = Registre::default();
        assert!(!sans_entree(&vide).2);
        assert!(sans_entree(&vide).1.starts_with("Aucun changement"));
        let arrete = Registre { historique_erreur: erreur.into(), historique_tronque: true, ..Registre::default() };
        assert_eq!(sans_entree(&arrete), ("L'historique ne s'est pas lu", erreur.to_string(), true));
        let attendu = format!("{erreur} Eve en a lu les 500 changements les plus récents.");
        assert_eq!(ce_qui_manque(&arrete, 500), Some(attendu));
        assert_eq!(ce_qui_manque(&arrete, 1), Some(format!("{erreur} Eve en a lu le changement le plus récent.")));
        let borne = Registre { historique_tronque: true, ..Registre::default() };
        assert!(ce_qui_manque(&borne, 1000).is_some_and(|t| t.starts_with("Eve a lu les 1\u{202f}000 changements")));
        assert_eq!(ce_qui_manque(&vide, 3), None);
    }

    #[test]
    fn windows_donne_l_heure_du_poste() {
        // L'instant même, par Windows, est l'heure locale du poste (GetLocalTime), à la seconde près.
        let unix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
        let local = par_windows(unix).expect("Windows convertit l'instant");
        // SAFETY : GetLocalTime remplit une structure qu'il rend par valeur ; aucun pointeur ne survit à l'appel.
        let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
        let jour = crate::centre::numero_du_jour(&format!("{:04}-{:02}-{:02}", t.wYear, t.wMonth, t.wDay)).unwrap();
        let attendu = jour * 86_400 + i64::from(t.wHour) * 3600 + i64::from(t.wMinute) * 60 + i64::from(t.wSecond);
        assert!((local - attendu).abs() <= 2, "{local} contre {attendu}");
        // Le décalage est un nombre entier de quarts d'heure : celui d'un fuseau, pas un écart de lecture.
        assert_eq!((local - unix).rem_euclid(900), 0, "un fuseau se compte en quarts d'heure");
    }
}
