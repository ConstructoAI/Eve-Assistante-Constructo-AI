//! Les listes de l'onglet « Comptabilité » : les factures des fournisseurs et celles des clients, comme les vues
//! « Factures » et « Clients » d'Entrepreneur Général (`comptabilite.html` vers 1960-2560), calculées par
//! [`crate::compta::listes`]. En tête, la recherche (un nom, un numéro, un montant au cent) et les filtres — la section
//! et la fiabilité des factures reçues, l'état, les annulées à part — ; dessous, les pastilles des filtres actifs,
//! chacune son × (celles qu'apporte un « aller à » du tableau de bord aussi) ; puis le tableau : ses en-têtes qui
//! trient (un clic, puis l'inverse), ses rangées dessinées seulement quand elles se voient (20 000 factures), son
//! en-tête et ses totaux collés au haut et au bas de l'écran tant qu'il continue au-delà — la page seule défile, comme
//! les sommaires (EG : `position: sticky`). La place manque : la TPS et la TVQ se fondent en « Taxes », puis les
//! textes se resserrent ; s'il défile encore de côté, le compte et le reste restent dessous, en vue. Étroite, la
//! fenêtre montre des cartes, les filtres derrière « Filtres », les totaux collés en bas, comme EG au téléphone.
//!
//! Un clic sur une facture ouvre son menu, comme une chose du planning : « En parler à Eve », « Me le rappeler
//! demain » — des demandes à Eve, jamais une écriture dans l'ERP.
//!
//! Le calcul ne se refait que quand ce qu'il lit change — la lecture (`lu_le`, le nombre de factures), les filtres, le
//! tri, le jour : il se garde dans la mémoire d'egui, comme les filtres et le tri de chaque liste, le temps que la
//! fenêtre vit. Rien ne s'écrit.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use eframe::egui::{self, Align2, Color32, FontFamily, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, pos2, vec2};
use eve::parole::normaliser;
use serde_json::Value;

use super::super::planning::etat_vide;
use super::super::theme::{self, BoutonErp, erp, pictos};
use super::super::{Fenetre, ICONES, SEMI};
use super::{Contexte, VueCompta};
use crate::compta::listes::{
    self as calcul, COLONNES_DES_CLIENTS, COLONNES_DES_FOURNISSEURS, Colonne, ETATS, FIABILITES, Fiabilite,
    FiltresDeListe, Liste, Paiement, Preparation, Reste, Totaux, Tri,
};
use crate::compta::{Cents, Cote, Facture, argent};

/// La hauteur d'une rangée du tableau, de son en-tête ; la marge d'une case ; la taille du texte.
const RANGEE: f32 = 36.0;
const TETE: f32 = 34.0;
const MARGE: f32 = 10.0;
const TAILLE: f32 = 13.5;
/// La hauteur d'une carte de la fenêtre étroite, l'écart compris.
const CARTE: f32 = 108.0;
const ECART_DES_CARTES: f32 = 8.0;
/// La hauteur de la rangée des sommes, au pied d'un tableau qui défile de côté (le compte et le reste, épinglés,
/// viennent dessous).
const HAUTEUR_DES_SOMMES: f32 = 35.0;

/// L'icône de la recherche (Search), celle de l'historique.
const CHERCHER: char = '\u{E721}';
/// Le × d'une pastille (Cancel), celui du bouton « Non ».
const OTER: char = '\u{E711}';
/// La coche d'un choix fait, et les chevrons du tri (ceux du Gantt : « Tout replier », « Tout déplier »).
const COCHE: char = '\u{E73E}';
const MONTE: char = '\u{E70E}';
const DESCEND: char = '\u{E70D}';

/// Ce qu'une liste retient, dans la mémoire d'egui : ses filtres, son tri, et (étroite) ses filtres ouverts.
#[derive(Clone, Debug, Default, PartialEq)]
struct EtatDeLaListe {
    filtres: FiltresDeListe,
    tri: Tri,
    filtres_ouverts: bool,
}

/// Ce qu'on choisit dans le menu d'une facture.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Choix {
    Parler,
    Rappeler,
}

fn id_de(quoi: &str, cote: Cote) -> egui::Id {
    egui::Id::unique(("compta-liste", quoi, cote))
}

fn empreinte(x: impl Hash) -> u64 {
    let mut h = DefaultHasher::new();
    x.hash(&mut h);
    h.finish()
}

fn normale(taille: f32) -> FontId {
    FontId::proportional(taille)
}

fn semi(taille: f32) -> FontId {
    FontId::new(taille, FontFamily::Name(SEMI.into()))
}

fn icones(taille: f32) -> FontId {
    FontId::new(taille, FontFamily::Name(ICONES.into()))
}

/// Une ligne de texte, coupée d'une ellipse à `largeur`.
fn ligne(p: &egui::Painter, texte: String, police: FontId, couleur: Color32, largeur: f32) -> Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(texte, police, couleur);
    job.wrap = egui::text::TextWrapping::truncate_at_width(largeur.max(1.0));
    p.layout_job(job)
}

/// Les factures préparées d'un côté (leurs textes pliés), refaites seulement quand la lecture change.
fn preparee(ctx: &egui::Context, c: &Contexte<'_>, cote: Cote) -> Arc<Preparation> {
    let id = id_de("preparation", cote);
    let cle = empreinte((cote, c.lecture.lu_le.as_str(), c.registre.factures.len()));
    if let Some((k, p)) = ctx.data(|d| d.get_temp::<(u64, Arc<Preparation>)>(id))
        && k == cle
    {
        return p;
    }
    let p = Arc::new(Preparation::de(c.registre, cote));
    ctx.data_mut(|d| d.insert_temp(id, (cle, Arc::clone(&p))));
    p
}

/// La liste calculée, refaite seulement quand la lecture, les filtres d'en tête ou de la liste, le tri ou le jour
/// changent.
fn calculee(ctx: &egui::Context, c: &Contexte<'_>, p: &Preparation, e: &EtatDeLaListe) -> Arc<Liste> {
    let cote = p.cote();
    let id = id_de("calcul", cote);
    let cle = empreinte((
        cote,
        c.lecture.lu_le.as_str(),
        c.registre.factures.len(),
        c.filtres,
        &e.filtres,
        e.tri,
        c.aujourdhui,
    ));
    if let Some((k, l)) = ctx.data(|d| d.get_temp::<(u64, Arc<Liste>)>(id))
        && k == cle
    {
        return l;
    }
    let l = Arc::new(calcul::calculer(c.registre, p, &c.filtres, &e.filtres, e.tri, c.aujourdhui));
    ctx.data_mut(|d| d.insert_temp(id, (cle, Arc::clone(&l))));
    l
}

/// La vue des factures d'un côté (`Cote::Fournisseur` : celles reçues ; `Cote::Client` : celles émises).
pub(super) fn page(f: &mut Fenetre, ui: &mut egui::Ui, c: &Contexte<'_>, cote: Cote) {
    let ctx = ui.ctx().clone();
    let vue = if cote == Cote::Fournisseur { VueCompta::Fournisseurs } else { VueCompta::Clients };
    let id_etat = id_de("etat", cote);
    let avant: EtatDeLaListe = ctx.data(|d| d.get_temp(id_etat)).unwrap_or_default();
    let mut etat = avant.clone();
    // Un « aller à » du tableau de bord : ses filtres remplacent ceux de la liste (le tri reste).
    if let Some(a) = super::prendre_l_aller(&ctx, vue) {
        etat.filtres = FiltresDeListe::de_l_aller(a.mois.as_deref(), a.section.as_deref(), a.tiers.as_deref(), a.etat);
        etat.filtres_ouverts = false;
    }
    let p = preparee(&ctx, c, cote);
    let mut l = calculee(&ctx, c, &p, &etat);
    let calcule_avec = etat.clone();
    barre(ui, &mut etat, &l, cote, c.etroite);
    pastilles(ui, &mut etat, cote);
    if etat != calcule_avec {
        l = calculee(&ctx, c, &p, &etat);
    }
    ui.add_space(12.0);
    let choix = if l.lignes.is_empty() && l.annulees.is_empty() {
        vide(ui, c, &mut etat, &l, cote);
        None
    } else if c.etroite {
        cartes(ui, c, &l, cote)
    } else {
        tableau(ui, c, &l, &mut etat, cote)
    };
    if let Some((choix, i)) = choix
        && let Some(facture) = c.registre.factures.get(i)
    {
        agir(f, facture, choix, c.aujourdhui);
    }
    if etat != avant {
        ctx.data_mut(|d| d.insert_temp(id_etat, etat));
        ctx.request_repaint();
    }
}

/// Une demande à Eve : en parler (comme si on l'avait écrite), ou le rappel de demain matin, que le chef pose sans
/// Claude. Jamais une écriture dans l'ERP.
fn agir(f: &Fenetre, facture: &Facture, choix: Choix, aujourdhui: &str) {
    match choix {
        Choix::Parler => f.geste("texte", Value::String(calcul::parler_de(facture, aujourdhui))),
        Choix::Rappeler => f.geste("rappel_demain", Value::String(calcul::rappel_de(facture, aujourdhui))),
    }
}

// ---- Les filtres --------------------------------------------------------------------------------------------------

/// La rangée des filtres : la recherche ; étroite, le bouton « Filtres » qui montre les autres ; la section et la
/// fiabilité (les factures reçues), l'état, les annulées.
fn barre(ui: &mut egui::Ui, e: &mut EtatDeLaListe, l: &Liste, cote: Cote, etroite: bool) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
        let largeur = if etroite { (ui.available_width() - 132.0).clamp(180.0, 420.0) } else { 380.0 };
        recherche(ui, &mut e.filtres.recherche, largeur, cote);
        if etroite {
            let n = e.filtres.actifs() - usize::from(!e.filtres.recherche.trim().is_empty());
            let mot = if n > 0 { format!("Filtres · {n}") } else { "Filtres".to_string() };
            let sorte = if e.filtres_ouverts { BoutonErp::Primaire } else { BoutonErp::Secondaire };
            let id = Some(id_de("bouton-des-filtres", cote));
            if theme::bouton_erp_nomme(ui, id, Some(pictos::PIPELINE), &mot, sorte)
                .on_hover_text("La section, la fiabilité, l'état, les annulées")
                .clicked()
            {
                e.filtres_ouverts = !e.filtres_ouverts;
            }
        }
        if !etroite || e.filtres_ouverts {
            if cote == Cote::Fournisseur {
                choix_de_section(ui, e, l, cote);
                choix_de_fiabilite(ui, e, l, cote);
            }
            choix_d_etat(ui, e, l, cote);
            bouton_des_annulees(ui, e, l, cote);
        }
    });
}

/// Le champ de la recherche (`largeur` : marges comprises, comme la compte egui) : la loupe, le texte, et un × qui
/// l'efface.
fn recherche(ui: &mut egui::Ui, texte: &mut String, largeur: f32, cote: Cote) {
    let id = id_de("recherche", cote);
    let champ = egui::TextEdit::singleline(texte)
        .id(id)
        .desired_width(largeur)
        .font(normale(14.0))
        .text_color(erp::TEXTE)
        .hint_text("Chercher un nom, un numéro, un montant")
        .frame(
            egui::Frame::new()
                .fill(Color32::WHITE)
                .stroke((1.0, erp::FILET_CHAMP))
                .corner_radius(4)
                .inner_margin(egui::Margin { left: 32, right: 30, top: 7, bottom: 7 }),
        );
    let rep = ui.add(champ);
    let r = rep.rect;
    let p = ui.painter();
    if rep.has_focus() {
        p.rect_stroke(r, 4.0, Stroke::new(1.5, erp::PRIMAIRE), StrokeKind::Inside);
    }
    p.text(pos2(r.left() + 16.0, r.center().y), Align2::CENTER_CENTER, CHERCHER, icones(13.0), erp::TEXTE_2);
    if rep.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        rep.surrender_focus();
    }
    if !texte.is_empty() {
        let croix = Rect::from_center_size(pos2(r.right() - 16.0, r.center().y), vec2(22.0, 22.0));
        let x = ui.interact(croix, id.with("effacer"), Sense::click());
        if x.hovered() {
            ui.painter().circle_filled(croix.center(), 10.0, erp::SURVOL);
        }
        let encre = if x.hovered() { erp::TITRE } else { erp::TEXTE_2 };
        ui.painter().text(croix.center(), Align2::CENTER_CENTER, OTER, icones(10.0), encre);
        if x.on_hover_text("Effacer la recherche").on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            texte.clear();
        }
    }
}

/// Un choix d'un menu de filtre : son mot, son compte (avec les autres filtres), et s'il est celui de la liste.
fn ligne_de_choix(ui: &mut egui::Ui, icone: char, mot: &str, n: Option<usize>, choisi: bool, id: egui::Id) -> bool {
    let texte = n.map_or_else(|| mot.to_string(), |n| format!("{mot} ({n})"));
    theme::ligne_du_menu(ui, if choisi { COCHE } else { icone }, &texte, false, id).clicked()
}

/// La section des factures reçues : celles de la période et du chantier, chacune son compte ; « Sans section ».
fn choix_de_section(ui: &mut egui::Ui, e: &mut EtatDeLaListe, l: &Liste, cote: Cote) {
    let actuelle = e.filtres.section.clone();
    let mot = match actuelle.as_deref().map(str::trim) {
        None => "Toutes les sections".to_string(),
        Some("") => "Sans section".to_string(),
        Some(s) => s.to_string(),
    };
    let rep =
        theme::bouton_erp_nomme(ui, Some(id_de("section", cote)), Some(pictos::LISTE), &mot, BoutonErp::Secondaire);
    let meme = |s: &str| actuelle.as_deref().is_some_and(|a| normaliser(a) == normaliser(s));
    theme::menu_blanc(&rep).show(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
            let id = |k: usize| egui::Id::unique(("compta-liste-section", cote, k));
            if ligne_de_choix(ui, pictos::LISTE, "Toutes les sections", None, actuelle.is_none(), id(0)) {
                e.filtres.section = None;
            }
            for (k, (s, n)) in l.sections.iter().enumerate() {
                if ligne_de_choix(ui, pictos::LISTE, s, Some(*n), meme(s), id(k + 1)) {
                    e.filtres.section = Some(s.clone());
                }
            }
            if let Some(n) = l.sans_section {
                let choisi = actuelle.as_deref().is_some_and(|a| a.trim().is_empty());
                if ligne_de_choix(ui, pictos::LISTE, "Sans section", Some(n), choisi, id(l.sections.len() + 1)) {
                    e.filtres.section = Some(String::new());
                }
            }
        });
    });
}

/// La fiabilité de la saisie des factures reçues : sûre, à vérifier, non jugée.
fn choix_de_fiabilite(ui: &mut egui::Ui, e: &mut EtatDeLaListe, l: &Liste, cote: Cote) {
    let mot = match e.filtres.fiabilite {
        None => "Fiabilité : toutes".to_string(),
        Some(x) => format!("Fiabilité : {}", x.mot().to_lowercase()),
    };
    let rep =
        theme::bouton_erp_nomme(ui, Some(id_de("fiabilite", cote)), Some(pictos::INFO), &mot, BoutonErp::Secondaire);
    theme::menu_blanc(&rep).show(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        let id = |k: usize| egui::Id::unique(("compta-liste-fiabilite", cote, k));
        if ligne_de_choix(ui, pictos::INFO, "Toutes", None, e.filtres.fiabilite.is_none(), id(0)) {
            e.filtres.fiabilite = None;
        }
        for (k, x) in FIABILITES.iter().enumerate() {
            if ligne_de_choix(
                ui,
                pictos::INFO,
                x.mot(),
                Some(l.fiabilites[k]),
                e.filtres.fiabilite == Some(*x),
                id(k + 1),
            ) {
                e.filtres.fiabilite = Some(*x);
            }
        }
    });
}

/// L'état : à recevoir (à payer), en retard, encaissées (payées), brouillons.
fn choix_d_etat(ui: &mut egui::Ui, e: &mut EtatDeLaListe, l: &Liste, cote: Cote) {
    let mot = e.filtres.etat.map_or("Tous les états", |x| x.mot(cote));
    let rep = theme::bouton_erp_nomme(ui, Some(id_de("etat", cote)), Some(pictos::RETARD), mot, BoutonErp::Secondaire);
    theme::menu_blanc(&rep).show(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        let id = |k: usize| egui::Id::unique(("compta-liste-etat", cote, k));
        if ligne_de_choix(ui, pictos::RETARD, "Tous les états", None, e.filtres.etat.is_none(), id(0)) {
            e.filtres.etat = None;
        }
        for (k, x) in ETATS.iter().enumerate() {
            if ligne_de_choix(ui, pictos::RETARD, x.mot(cote), Some(l.etats[k]), e.filtres.etat == Some(*x), id(k + 1))
            {
                e.filtres.etat = Some(*x);
            }
        }
    });
}

/// Les annulées (« Retirées » dans EG) : montrées à part, après les autres, jamais comptées. Le bouton dit combien
/// répondent aux filtres ; sans aucune, il s'éteint.
fn bouton_des_annulees(ui: &mut egui::Ui, e: &mut EtatDeLaListe, l: &Liste, cote: Cote) {
    let n = l.n_annulees;
    let montrees = e.filtres.annulees;
    // Montrées, la coche ; cachées, la corbeille des cartes retirées (EG les dit « Retirées »).
    let (icone, mot) = if montrees {
        (COCHE, format!("Annulées montrées · {}", nombre(n as u64)))
    } else {
        (pictos::RETIRER, format!("Annulées · {}", nombre(n as u64)))
    };
    let sorte = if n == 0 && !montrees { BoutonErp::Fantome } else { BoutonErp::Secondaire };
    let rep = theme::bouton_erp_nomme(ui, Some(id_de("annulees", cote)), Some(icone), &mot, sorte);
    let aide = if montrees {
        "Les factures annulées sont montrées à part, après les autres ; elles ne comptent jamais. Un clic les cache."
    } else {
        "Montrer les factures annulées, à part, après les autres ; elles ne comptent jamais."
    };
    if rep.on_hover_text(aide).clicked() && (n > 0 || montrees) {
        e.filtres.annulees = !montrees;
    }
}

/// Les pastilles des filtres actifs, chacune son × ; « Effacer les filtres » dès qu'il y en a deux.
fn pastilles(ui: &mut egui::Ui, e: &mut EtatDeLaListe, cote: Cote) {
    let toutes = e.filtres.pastilles(cote);
    if toutes.is_empty() {
        return;
    }
    ui.add_space(8.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 6.0);
        for (k, (quoi, texte)) in toutes.iter().enumerate() {
            if pastille(ui, texte, egui::Id::unique(("compta-liste-pastille", cote, k))) {
                e.filtres.oter(*quoi);
            }
        }
        if toutes.len() > 1
            && theme::bouton_erp_nomme(
                ui,
                Some(id_de("effacer", cote)),
                None,
                "Effacer les filtres",
                BoutonErp::Fantome,
            )
            .clicked()
        {
            e.filtres = e.filtres.effaces();
        }
    });
}

/// Une pastille : son texte sur un lavis bleu pâle, et son × ; rend vrai quand on l'ôte.
fn pastille(ui: &mut egui::Ui, texte: &str, id: egui::Id) -> bool {
    let p = ui.painter().clone();
    let g = p.layout_no_wrap(texte.to_string(), semi(12.5), erp::TITRE);
    let (r, _) = ui.allocate_exact_size(vec2(12.0 + g.size().x + 34.0, 28.0), Sense::hover());
    let croix = Rect::from_center_size(pos2(r.right() - 16.0, r.center().y), vec2(22.0, 22.0));
    let rep = ui.interact(croix, id, Sense::click());
    p.rect(r, 14.0, erp::BLEU.pastel.gamma_multiply(0.18), Stroke::new(1.0, erp::BLEU.pastel), StrokeKind::Inside);
    let h = g.size().y;
    p.galley(pos2(r.left() + 12.0, r.center().y - h / 2.0), g, erp::TITRE);
    if rep.hovered() {
        p.circle_filled(croix.center(), 10.0, Color32::WHITE);
    }
    let encre = if rep.hovered() { erp::TITRE } else { erp::TEXTE_2 };
    p.text(croix.center(), Align2::CENTER_CENTER, OTER, icones(9.5), encre);
    rep.on_hover_text("Ôter ce filtre").on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

/// Une liste vide le dit, et pourquoi ; avec des filtres, de quoi les effacer.
fn vide(ui: &mut egui::Ui, c: &Contexte<'_>, e: &mut EtatDeLaListe, l: &Liste, cote: Cote) {
    // À la française : un montant ou des guillemets ne se coupent pas en fin de ligne.
    let pourquoi = super::super::insecables(&calcul::pourquoi_vide(c.registre, cote, &c.filtres, &e.filtres, l));
    let bouton = (e.filtres.actifs() > 0).then_some("Effacer les filtres");
    let icone = if cote == Cote::Fournisseur { pictos::FACTURES_RECUES } else { pictos::RECEVOIR };
    if etat_vide(ui, icone, erp::BLEU, "Aucune facture ici", &pourquoi, bouton) {
        e.filtres = e.filtres.effaces();
    }
}

// ---- Les cases ------------------------------------------------------------------------------------------------------

/// Ce que montre une case.
enum Contenu {
    Rien,
    Texte { texte: String, couleur: Color32, fort: bool },
    Montant { texte: String, couleur: Color32, fort: bool },
    Badge { texte: String, teinte: erp::Teinte },
}

fn texte(t: &str, couleur: Color32, fort: bool) -> Contenu {
    let t = t.trim();
    if t.is_empty() { Contenu::Rien } else { Contenu::Texte { texte: t.to_string(), couleur, fort } }
}

/// Un montant signé (une note de crédit, négatif) ; « — » quand la base n'en a pas.
fn montant(m: Option<Cents>, signe: Cents, fort: bool) -> Contenu {
    match m {
        Some(_) => Contenu::Montant { texte: argent(signe), couleur: if fort { erp::TITRE } else { erp::TEXTE }, fort },
        None => Contenu::Montant { texte: "\u{2014}".into(), couleur: erp::TEXTE_3, fort: false },
    }
}

/// Le statut d'une facture : un brouillon et une annulée en badge gris, le reste en clair.
fn statut(f: &Facture) -> Contenu {
    if f.annulee() || f.brouillon() {
        Contenu::Badge { texte: calcul::statut_dit(f), teinte: erp::GRIS }
    } else {
        texte(&calcul::statut_dit(f), erp::TEXTE_2, false)
    }
}

/// La fiabilité d'une facture reçue : « À vérifier » en badge, « Sûre » en clair, « Non jugée » pâle.
fn fiabilite(f: &Facture) -> Contenu {
    match Fiabilite::de(f) {
        Fiabilite::Sure => texte(Fiabilite::Sure.mot(), erp::TEXTE_2, false),
        Fiabilite::AVerifier => Contenu::Badge { texte: Fiabilite::AVerifier.mot().into(), teinte: erp::ORANGE },
        Fiabilite::NonJugee => texte(Fiabilite::NonJugee.mot(), erp::TEXTE_3, false),
        Fiabilite::Autre => {
            texte(&crate::planning::en_francais(f.fiabilite.as_deref().unwrap_or("")), erp::TEXTE_2, false)
        }
    }
}

fn contenu(f: &Facture, col: Colonne, aujourdhui: &str) -> Contenu {
    let client = f.cote == Cote::Client;
    match col {
        Colonne::Date => texte(f.date.as_deref().unwrap_or(""), erp::TEXTE_2, false),
        Colonne::Tiers => texte(&f.tiers, erp::TITRE, !client),
        Colonne::Numero => texte(calcul::numero_dit(f), if client { erp::TITRE } else { erp::TEXTE }, client),
        Colonne::Chantier => texte(f.projet.as_deref().unwrap_or(""), erp::TEXTE, false),
        Colonne::Section => texte(f.section.as_deref().unwrap_or(""), erp::TEXTE_2, false),
        Colonne::AvantTaxes => montant(f.sous_total, f.sous_total_signe(), false),
        Colonne::Tps => montant(f.tps, f.tps_signee(), false),
        Colonne::Tvq => montant(f.tvq, f.tvq_signee(), false),
        Colonne::Taxes => montant(f.tps.or(f.tvq), f.tps_signee() + f.tvq_signee(), false),
        Colonne::Total => montant(f.total, f.total_signe(), true),
        Colonne::Reste => match calcul::reste_du_fournisseur(f, aujourdhui) {
            Reste::APayer(c, j) => Contenu::Montant {
                texte: argent(c),
                couleur: if j > 0 { erp::ECART_MAUVAIS } else { erp::TEXTE },
                fort: j > 0,
            },
            Reste::Payee(d) => Contenu::Texte {
                texte: d.map_or_else(|| "Payée".to_string(), |d| format!("Payée le {d}")),
                couleur: erp::SUCCES_ENCRE,
                fort: false,
            },
            Reste::Rien => Contenu::Rien,
        },
        Colonne::Paiement => match calcul::paiement_du_client(f, aujourdhui) {
            Paiement::Encaissee(d) => Contenu::Texte {
                texte: d.map_or_else(|| "Encaissée".to_string(), |d| format!("Encaissée le {d}")),
                couleur: erp::SUCCES_ENCRE,
                fort: false,
            },
            Paiement::ARecevoir(c) => {
                Contenu::Badge { texte: format!("À recevoir {}", argent(c)), teinte: erp::JAUNE }
            }
            Paiement::EnRetard(j, _) => Contenu::Badge { texte: format!("En retard · {j} j"), teinte: erp::ROUGE },
            Paiement::Credit => Contenu::Badge { texte: "Crédit".into(), teinte: erp::BLEU },
            Paiement::Soldee => Contenu::Texte { texte: "Soldée".into(), couleur: erp::SUCCES_ENCRE, fort: false },
            Paiement::MarqueePayee => {
                Contenu::Texte { texte: "Marquée payée".into(), couleur: erp::SUCCES_ENCRE, fort: false }
            }
            Paiement::Rien => Contenu::Rien,
        },
        Colonne::Fiabilite => fiabilite(f),
        Colonne::Statut => statut(f),
    }
}

/// Une case du tableau : son texte coupé à sa largeur, un montant à droite ; une annulée, pâle et ses montants barrés.
/// Rend ce que dirait sa bulle : son texte entier s'il est coupé ; le retard d'une facture qui en a.
fn cellule(
    p: &egui::Painter,
    case: Rect,
    f: &Facture,
    col: Colonne,
    aujourdhui: &str,
    annulee: bool,
) -> Option<String> {
    let dedans = case.width() - 2.0 * MARGE;
    let pale = |c: Color32| if annulee { erp::TEXTE_3 } else { c };
    let a_droite = col.en_montant();
    let poser = |g: Arc<egui::Galley>, couleur: Color32, barre: bool| {
        let (l, h) = (g.size().x, g.size().y);
        let x = if a_droite { case.right() - MARGE - l } else { case.left() + MARGE };
        let y = case.center().y - h / 2.0;
        p.galley(pos2(x, y), g, couleur);
        if barre {
            p.hline(x..=x + l, y + h / 2.0, Stroke::new(1.0, couleur));
        }
    };
    let mut bulle = None;
    match contenu(f, col, aujourdhui) {
        Contenu::Rien => {}
        Contenu::Texte { texte, couleur, fort } | Contenu::Montant { texte, couleur, fort } => {
            let police = if fort { semi(TAILLE) } else { normale(TAILLE) };
            let g = ligne(p, texte.clone(), police, pale(couleur), dedans);
            if g.elided {
                bulle = Some(texte);
            }
            poser(g, pale(couleur), annulee && col.en_montant());
        }
        Contenu::Badge { texte, .. } if annulee => {
            poser(ligne(p, texte, normale(TAILLE), erp::TEXTE_3, dedans), erp::TEXTE_3, false);
        }
        Contenu::Badge { texte, teinte } => {
            let g = ligne(p, texte.clone(), semi(11.5), teinte.encre, dedans - 18.0);
            if g.elided {
                bulle = Some(texte);
            }
            let taille = g.size() + vec2(18.0, 7.0);
            let x = if a_droite { case.right() - MARGE - taille.x } else { case.left() + MARGE };
            let place = Rect::from_min_size(pos2(x, case.center().y - taille.y / 2.0), taille);
            theme::peindre_badge(p, place, g, teinte);
        }
    }
    // Le retard d'une facture : depuis quand elle est échue, et ce qui reste.
    let retard = f.jours_de_retard(aujourdhui);
    if matches!(col, Colonne::Reste | Colonne::Paiement) && retard > 0 && !annulee {
        let echeance = f.echeance_effective().unwrap_or_default();
        let jours = if retard > 1 { format!("{retard} jours") } else { "1 jour".to_string() };
        bulle = Some(format!("En retard de {jours} : échue le {echeance}, reste {}", argent(f.reste())));
    }
    bulle
}

// ---- Le tableau ---------------------------------------------------------------------------------------------------

/// Une colonne à poser : sa largeur à l'aise, sa largeur la plus serrée (un texte s'y coupe d'une ellipse, sa bulle le
/// dit entier ; un montant, une date, un badge y tiennent encore), sa part de la place qui reste.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Gabarit {
    colonne: Colonne,
    aise: f32,
    serre: f32,
    part: f32,
}

/// Le gabarit d'une colonne ; `pied` : la largeur de sa somme au pied, sous laquelle un montant ne descend jamais.
fn gabarit(colonne: Colonne, cote: Cote, pied: f32) -> Gabarit {
    let client = cote == Cote::Client;
    let (aise, serre, part): (f32, f32, f32) = match colonne {
        Colonne::Date => (96.0, 90.0, 0.0),
        Colonne::Tiers => (if client { 190.0 } else { 170.0 }, 104.0, 3.0),
        Colonne::Numero => (if client { 150.0 } else { 130.0 }, 84.0, 1.0),
        Colonne::Chantier => (if client { 170.0 } else { 150.0 }, 92.0, 2.0),
        Colonne::Section => (160.0, 84.0, 2.0),
        Colonne::AvantTaxes => (112.0, 104.0, 0.0),
        Colonne::Tps => (92.0, 88.0, 0.0),
        Colonne::Tvq => (96.0, 92.0, 0.0),
        Colonne::Taxes => (112.0, 104.0, 0.0),
        Colonne::Total => (120.0, 110.0, 0.0),
        // « Payée le 2026-10-31 », « À recevoir 53 536,93 $ » : en entier.
        Colonne::Reste => (156.0, 146.0, 0.0),
        Colonne::Paiement => (200.0, 176.0, 0.0),
        Colonne::Fiabilite => (100.0, 96.0, 0.0),
        Colonne::Statut => (if client { 190.0 } else { 160.0 }, 100.0, 1.0),
    };
    Gabarit { colonne, aise: aise.max(pied), serre: serre.max(pied), part }
}

/// Les colonnes d'un côté posées dans `dispo` (leur ordre : celui du calcul) : toutes à l'aise, la place qui reste aux
/// textes selon leur part ; sinon la TPS et la TVQ fondues en « Taxes », comme chez les clients ; sinon les colonnes
/// resserrées, chacune selon ce qu'elle peut céder, pour tenir tout juste ; sinon toutes au plus serré, et le tableau
/// défile de côté. `pied` : la largeur de la somme d'une colonne au pied.
fn disposer(cote: Cote, pied: impl Fn(Colonne) -> f32, dispo: f32) -> Vec<(Colonne, f32)> {
    let ordre: &[Colonne] = if cote == Cote::Fournisseur { &COLONNES_DES_FOURNISSEURS } else { &COLONNES_DES_CLIENTS };
    let poser = |fondre: bool| -> Vec<Gabarit> {
        let mut g = Vec::new();
        for &c in ordre {
            match c {
                Colonne::Tps if fondre => g.push(gabarit(Colonne::Taxes, cote, pied(Colonne::Taxes))),
                Colonne::Tvq if fondre => {}
                _ => g.push(gabarit(c, cote, pied(c))),
            }
        }
        g
    };
    let aise = |g: &[Gabarit]| g.iter().map(|x| x.aise).sum::<f32>();
    let etaler = |g: Vec<Gabarit>| -> Vec<(Colonne, f32)> {
        let parts: f32 = g.iter().map(|x| x.part).sum();
        let reste = (dispo - aise(&g)).max(0.0);
        g.into_iter().map(|x| (x.colonne, x.aise + if parts > 0.0 { reste * x.part / parts } else { 0.0 })).collect()
    };
    let complet = poser(false);
    if aise(&complet) <= dispo {
        return etaler(complet);
    }
    let fondu = poser(true);
    if aise(&fondu) <= dispo {
        return etaler(fondu);
    }
    let manque = aise(&fondu) - dispo;
    let souple: f32 = fondu.iter().map(|x| x.aise - x.serre).sum();
    let k = if souple > 0.0 { (manque / souple).min(1.0) } else { 1.0 };
    fondu.into_iter().map(|x| (x.colonne, x.aise - (x.aise - x.serre) * k)).collect()
}

/// Les largeurs des colonnes d'un côté dans `dispo`, celles des sommes du pied mesurées.
fn largeurs(ui: &egui::Ui, cote: Cote, t: &Totaux, dispo: f32) -> Vec<(Colonne, f32)> {
    let p = ui.painter();
    let mesure = |c: Cents| p.layout_no_wrap(argent(c), semi(TAILLE), erp::TITRE).size().x + 2.0 * MARGE + 6.0;
    let s = &t.comptees;
    let pied = |c: Colonne| match c {
        Colonne::AvantTaxes => mesure(s.sous_total),
        Colonne::Tps => mesure(s.tps),
        Colonne::Tvq => mesure(s.tvq),
        Colonne::Taxes => mesure(s.tps + s.tvq),
        Colonne::Total => mesure(s.total),
        _ => 0.0,
    };
    disposer(cote, pied, dispo)
}

/// Où se peignent l'en-tête et le pied d'un tableau qui commence à `haut` (son en-tête de `tete` points, `corps` points
/// de rangées, son pied de `pied` points) dans l'écran `(haut, bas)` : à leur place, ou contre le haut et le bas de
/// l'écran tant que le tableau continue au-delà (EG : `position: sticky`). `reserve` : ce qui se colle au bas de
/// l'écran sous le pied (le compte et le reste épinglés d'un tableau qui défile de côté). Rend `(en-tête, pied)`.
fn collants(haut: f32, tete: f32, corps: f32, pied: f32, reserve: f32, ecran: (f32, f32)) -> (f32, f32) {
    let naturel = haut + tete + corps;
    let y_pied = naturel.min(ecran.1 - reserve - pied).max(haut + tete);
    let y_tete = haut.max(ecran.0).min(naturel - tete).max(haut);
    (y_tete, y_pied)
}

/// Le tableau, dans une carte : son en-tête, ses rangées à l'écran, ses totaux. La page seule défile : l'en-tête et le
/// pied se collent au haut et au bas de l'écran tant que le tableau continue au-delà. Plus large que la carte (même
/// resserré), il défile de côté — le compte et le reste, eux, restent dessous, épinglés, toujours en vue. Rend ce qu'on
/// a choisi dans le menu d'une facture.
fn tableau(
    ui: &mut egui::Ui,
    c: &Contexte<'_>,
    l: &Liste,
    e: &mut EtatDeLaListe,
    cote: Cote,
) -> Option<(Choix, usize)> {
    let mut choix = None;
    theme::carte(ui, None, 0.0, |ui| {
        let dispo = ui.available_width();
        let cols = largeurs(ui, cote, &l.totaux, dispo);
        let largeur: f32 = cols.iter().map(|c| c.1).sum();
        if largeur <= dispo + 0.5 {
            choix = table(ui, c, l, e, cote, &cols, true, 0.0);
            return;
        }
        let h_epingle = hauteur_du_pied(&l.totaux, cote);
        let sortie = egui::ScrollArea::horizontal()
            .id_salt(("compta-liste-large", cote))
            .auto_shrink([false, true])
            .show(ui, |ui| table(ui, c, l, e, cote, &cols, false, h_epingle));
        choix = sortie.inner;
        // Plus de colonnes à droite : le bord s'estompe.
        let vu = sortie.inner_rect;
        if sortie.state.clamped_offset().x + vu.width() < sortie.content_size.x - 1.0 {
            let ecran = ui.clip_rect();
            let haut = vu.top().max(ecran.top());
            let bas = vu.bottom().min(ecran.bottom());
            if bas > haut {
                estomper(ui.painter(), Rect::from_min_max(pos2(vu.right() - 36.0, haut), pos2(vu.right(), bas)));
            }
        }
        epingle(ui, &l.totaux, cote, h_epingle, vu.top());
    });
    choix
}

/// Le bord droit de ce qui défile de côté, qui s'estompe dans le blanc de la carte : il y a d'autres colonnes.
fn estomper(p: &egui::Painter, r: Rect) {
    let mut m = egui::Mesh::default();
    m.colored_vertex(r.left_top(), Color32::TRANSPARENT);
    m.colored_vertex(r.right_top(), Color32::WHITE);
    m.colored_vertex(r.right_bottom(), Color32::WHITE);
    m.colored_vertex(r.left_bottom(), Color32::TRANSPARENT);
    m.add_triangle(0, 1, 2);
    m.add_triangle(0, 2, 3);
    p.add(egui::Shape::mesh(m));
}

/// Le tableau à sa largeur (`cols`) : les rangées à l'écran (les montrées, puis le titre des annulées et les annulées),
/// le pied puis l'en-tête par-dessus, collés à l'écran. `entier` : le pied porte aussi le compte et le reste (sinon,
/// épinglés dessous, ils prennent `reserve` points au bas de l'écran).
#[allow(clippy::too_many_arguments)]
fn table(
    ui: &mut egui::Ui,
    c: &Contexte<'_>,
    l: &Liste,
    e: &mut EtatDeLaListe,
    cote: Cote,
    cols: &[(Colonne, f32)],
    entier: bool,
    reserve: f32,
) -> Option<(Choix, usize)> {
    let largeur: f32 = cols.iter().map(|c| c.1).sum();
    ui.set_width(largeur);
    let taxes = noms_des_taxes(ui.ctx(), c);
    let n_lignes = l.lignes.len();
    let n = n_lignes + if l.annulees.is_empty() { 0 } else { 1 + l.annulees.len() };
    let h_pied = if entier { hauteur_du_pied(&l.totaux, cote) } else { HAUTEUR_DES_SOMMES };
    let corps = n as f32 * RANGEE;
    let (zone, _) = ui.allocate_exact_size(vec2(largeur, TETE + corps + h_pied), Sense::hover());
    let ecran = ui.clip_rect();
    let (y_tete, y_pied) = collants(zone.top(), TETE, corps, h_pied, reserve, (ecran.top(), ecran.bottom()));
    let haut_du_corps = zone.top() + TETE;
    let mut choix = None;
    // Les rangées à l'écran seulement ; une rangée que couvrent l'en-tête ou le pied ne répond pas à la souris (le clic
    // irait à une rangée cachée).
    let premiere = (((ecran.top() - haut_du_corps) / RANGEE).floor().max(0.0) as usize).min(n);
    let derniere = (((ecran.bottom() - haut_du_corps) / RANGEE).ceil().max(0.0) as usize).min(n);
    for k in premiere..derniere {
        let r = Rect::from_min_size(pos2(zone.left(), haut_du_corps + k as f32 * RANGEE), vec2(largeur, RANGEE));
        let visible =
            Rect::from_min_max(pos2(r.left(), r.top().max(y_tete + TETE)), pos2(r.right(), r.bottom().min(y_pied)));
        let rangee = if k < n_lignes {
            Some((l.lignes[k], false))
        } else if k == n_lignes {
            titre_des_annulees(ui.painter(), r, l.annulees.len());
            None
        } else {
            l.annulees.get(k - n_lignes - 1).map(|i| (*i, true))
        };
        if let Some((i, annulee)) = rangee
            && let Some(x) = rangee_du_tableau(ui, c, i, cols, annulee, cote, r, visible)
        {
            choix = Some(x);
        }
    }
    // Le pied, puis l'en-tête, par-dessus les rangées.
    let p = ui.painter().clone();
    pied(&p, Rect::from_min_size(pos2(zone.left(), y_pied), vec2(largeur, h_pied)), cols, &l.totaux, cote, entier);
    let r_tete = Rect::from_min_size(pos2(zone.left(), y_tete), vec2(largeur, TETE));
    if let Some(col) = entete(ui, r_tete, cols, e.tri, cote, &taxes) {
        e.tri = e.tri.clic(col);
    }
    choix
}

/// Les noms des deux taxes des factures, pour les en-têtes (la règle du contrat, `compta::noms_des_taxes`), gardés le
/// temps d'une lecture : 20 000 factures ne se recomptent pas à chaque image.
fn noms_des_taxes(ctx: &egui::Context, c: &Contexte<'_>) -> std::sync::Arc<(String, String)> {
    type EnMemoire = ((String, usize), std::sync::Arc<(String, String)>);
    let id = egui::Id::unique("compta-liste-noms-des-taxes");
    let cle = (c.lecture.lu_le.clone(), c.registre.factures.len());
    if let Some((k, v)) = ctx.data(|d| d.get_temp::<EnMemoire>(id))
        && k == cle
    {
        return v;
    }
    let v = std::sync::Arc::new(crate::compta::noms_des_taxes(c.registre));
    ctx.data_mut(|d| d.insert_temp::<EnMemoire>(id, (cle, v.clone())));
    v
}

/// L'en-tête, peint à sa place `r` (collé au haut de l'écran quand le tableau défile) : chaque titre trie d'un clic
/// (puis l'inverse) ; la colonne triée, à l'encre des titres, son chevron de l'accent ; « Taxes » porte le tri de la
/// TPS ou de la TVQ qu'elle a fondues. Rend la colonne cliquée.
fn entete(
    ui: &mut egui::Ui,
    r: Rect,
    cols: &[(Colonne, f32)],
    tri: Tri,
    cote: Cote,
    taxes: &(String, String),
) -> Option<Colonne> {
    let p = ui.painter().clone();
    p.rect_filled(r, 6.0, erp::FILET_PALE);
    let mut x = r.left();
    let mut clic = None;
    for (k, (col, l)) in cols.iter().enumerate() {
        let case = Rect::from_min_size(pos2(x, r.top()), vec2(*l, TETE));
        x += l;
        let rep = ui.interact(case, egui::Id::unique(("compta-liste-entete", cote, k)), Sense::click());
        let trie =
            tri.colonne == *col || (*col == Colonne::Taxes && matches!(tri.colonne, Colonne::Tps | Colonne::Tvq));
        let encre = if trie || rep.hovered() { erp::TITRE } else { erp::TEXTE_2 };
        let fleche = if trie { 16.0 } else { 0.0 };
        // Les deux taxes sous LEURS noms (une TVH, une « Sales Tax ») ; une seconde absente : « — ».
        let titre = match col {
            Colonne::Tps => taxes.0.clone(),
            Colonne::Tvq if taxes.1.is_empty() => "\u{2014}".to_string(),
            Colonne::Tvq => taxes.1.clone(),
            _ => col.titre(cote).to_string(),
        };
        let g = ligne(&p, titre, semi(12.5), encre, l - 2.0 * MARGE - fleche);
        let (lg, h) = (g.size().x, g.size().y);
        let gauche = if col.en_montant() { case.right() - MARGE - lg - fleche } else { case.left() + MARGE };
        p.galley(pos2(gauche, case.center().y - h / 2.0), g, encre);
        if trie {
            let chevron = if tri.descendant { DESCEND } else { MONTE };
            p.text(
                pos2(gauche + lg + 10.0, case.center().y),
                Align2::CENTER_CENTER,
                chevron,
                icones(10.0),
                erp::PRIMAIRE,
            );
        }
        let aide = if trie {
            if tri.descendant {
                "Trié du plus grand au plus petit ; un clic inverse"
            } else {
                "Trié du plus petit au plus grand ; un clic inverse"
            }
        } else {
            "Trier par cette colonne"
        };
        if rep.on_hover_text(aide).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            clic = Some(*col);
        }
    }
    clic
}

/// Le titre des annulées, montrées à part, après les autres, peint à sa place `r`.
fn titre_des_annulees(p: &egui::Painter, r: Rect, n: usize) {
    p.hline(r.x_range(), r.top() + 6.0, Stroke::new(1.0, erp::FILET_BOUTON));
    p.text(
        pos2(r.left() + MARGE, r.center().y + 3.0),
        Align2::LEFT_CENTER,
        titre_d_annulees(n),
        semi(12.5),
        erp::TEXTE_2,
    );
}

/// « 1 facture annulée — montrée à part, jamais comptée », « 3 factures annulées — montrées… ».
fn titre_d_annulees(n: usize) -> String {
    if n > 1 {
        format!("{} factures annulées — montrées à part, jamais comptées", nombre(n as u64))
    } else {
        "1 facture annulée — montrée à part, jamais comptée".to_string()
    }
}

/// Une rangée du tableau, à sa place `r` : ses cases ; au survol, et menu ouvert, un lavis gris ; un clic, son menu.
/// Elle ne répond à la souris que dans `visible` (hors de l'en-tête et du pied collés), et plus du tout s'il en reste
/// moins de la moitié.
#[allow(clippy::too_many_arguments)]
fn rangee_du_tableau(
    ui: &mut egui::Ui,
    c: &Contexte<'_>,
    i: usize,
    cols: &[(Colonne, f32)],
    annulee: bool,
    cote: Cote,
    r: Rect,
    visible: Rect,
) -> Option<(Choix, usize)> {
    let f = c.registre.factures.get(i)?;
    let sens = if visible.height() >= RANGEE / 2.0 { Sense::click() } else { Sense::hover() };
    let visible = if visible.height() > 0.0 { visible } else { Rect::NOTHING };
    let rep = ui.interact(visible, egui::Id::unique(("compta-liste-facture", cote, f.id)), sens);
    let ouvert = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&rep));
    let p = ui.painter().clone();
    if rep.hovered() || ouvert {
        p.rect_filled(r.shrink2(vec2(0.0, 1.0)), 6.0, erp::SURVOL);
    }
    p.hline(r.x_range(), r.bottom() - 0.5, Stroke::new(1.0, erp::FILET_DE_CARTE));
    // La bulle de la case sous le pointeur : son texte entier s'il est coupé, le retard d'une facture qui en a.
    let pointeur = rep.hover_pos().map(|q| q.x);
    let mut bulle = None;
    let mut x = r.left();
    for (col, l) in cols {
        let case = Rect::from_min_size(pos2(x, r.top()), vec2(*l, RANGEE));
        let dit = cellule(&p, case, f, *col, c.aujourdhui, annulee);
        if pointeur.is_some_and(|px| case.x_range().contains(px)) {
            bulle = dit;
        }
        x += l;
    }
    let mut rep = rep.on_hover_cursor(egui::CursorIcon::PointingHand);
    if let Some(b) = bulle {
        rep = rep.on_hover_text(b);
    }
    menu_de_la_facture(&rep, cote, f.id).map(|x| (x, i))
}

/// Le menu d'une facture, qu'ouvre un clic sur elle, là où l'on a touché (une rangée fait toute la largeur) : « En
/// parler à Eve », « Me le rappeler demain ».
fn menu_de_la_facture(rep: &egui::Response, cote: Cote, id: i64) -> Option<Choix> {
    let touchee = egui::Id::unique(("compta-liste-touchee", cote, id));
    if rep.clicked()
        && let Some(p) = rep.interact_pointer_pos()
    {
        rep.ctx.data_mut(|d| d.insert_temp(touchee, p));
    }
    let mut menu = theme::menu_blanc(rep);
    if let Some(p) = rep.ctx.data(|d| d.get_temp::<Pos2>(touchee)) {
        menu = menu.at_position(p);
    }
    let mut choisi = None;
    menu.show(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        let ligne = |k: usize| egui::Id::unique(("compta-liste-menu", cote, id, k));
        if theme::ligne_du_menu(ui, pictos::PARLER, "En parler à Eve", false, ligne(0)).clicked() {
            choisi = Some(Choix::Parler);
        }
        if theme::ligne_du_menu(ui, pictos::RAPPELS, "Me le rappeler demain", false, ligne(1)).clicked() {
            choisi = Some(Choix::Rappeler);
        }
    });
    choisi
}

/// « 12 factures », « 1 facture », « 13 566 factures ».
fn factures(n: u32) -> String {
    crate::compta::bord::pluriel(n, "facture", "factures")
}

/// Un nombre, une espace fine entre les milliers, comme l'argent : « 13 566 » (la QA5, passe 2 : « Total : 13566
/// factures » sous un bandeau qui disait « 20 000 »).
fn nombre(n: u64) -> String {
    crate::compta::par_milliers(n)
}

/// Les montrées qui ne comptent pas, en clair : « + 2 non comptées (1 brouillon, 1 pro forma) : 21 730,28 $ ».
fn non_comptees(t: &Totaux) -> Option<String> {
    let s = &t.non_comptees;
    if s.n == 0 {
        return None;
    }
    let mut quoi = Vec::new();
    match t.brouillons_non_comptes {
        0 => {}
        1 => quoi.push("1 brouillon".to_string()),
        n => quoi.push(format!("{} brouillons", nombre(n as u64))),
    }
    if t.pro_forma > 0 {
        quoi.push(format!("{} pro forma", nombre(t.pro_forma as u64)));
    }
    let mot = if s.n > 1 { "non comptées" } else { "non comptée" };
    Some(format!("+ {} {mot} ({}) : {}", nombre(s.n as u64), quoi.join(", "), argent(s.total)))
}

/// Les lignes du pied, à gauche : le nombre de factures qui comptent ; celles montrées qui ne comptent pas ; les
/// annulées ; celles sans total.
fn compte_du_pied(t: &Totaux) -> Vec<(String, bool)> {
    let mut lignes = vec![(format!("Total : {}", factures(t.comptees.n)), true)];
    if let Some(x) = non_comptees(t) {
        lignes.push((x, false));
    }
    match t.annulees {
        0 => {}
        1 => lignes.push(("+ 1 annulée, non comptée".to_string(), false)),
        n => lignes.push((format!("+ {} annulées, non comptées", nombre(n as u64)), false)),
    }
    if t.comptees.sans_total > 0 {
        lignes.push((format!("{} sans total", nombre(t.comptees.sans_total as u64)), false));
    }
    lignes
}

/// Les lignes du pied, à droite : l'encaissé et le reste à recevoir (les clients), le reste à payer (les fournisseurs),
/// et ce qui en est en retard.
fn reste_du_pied(t: &Totaux, cote: Cote) -> Vec<(String, Color32)> {
    let mut lignes = Vec::new();
    match cote {
        Cote::Client => {
            lignes.push((format!("Encaissé {}", argent(t.encaisse)), erp::SUCCES_ENCRE));
            lignes.push((format!("À recevoir {}", argent(t.reste)), erp::TITRE));
        }
        Cote::Fournisseur => lignes.push((format!("Reste à payer {}", argent(t.reste)), erp::TITRE)),
    }
    if t.n_en_retard > 0 {
        let qui = if t.n_en_retard > 1 {
            format!("{} en retard", nombre(t.n_en_retard as u64))
        } else {
            "1 en retard".to_string()
        };
        lignes.push((format!("dont {qui} : {}", argent(t.en_retard)), erp::ECART_MAUVAIS));
    }
    lignes
}

/// La hauteur du pied : une ligne de 19 points par ligne de sa plus longue colonne.
fn hauteur_du_pied(t: &Totaux, cote: Cote) -> f32 {
    let lignes = compte_du_pied(t).len().max(reste_du_pied(t, cote).len()).max(1);
    16.0 + 19.0 * lignes as f32
}

/// Le fond d'un pied collé : opaque (des rangées passent dessous), un filet qui le sépare d'elles.
fn fond_du_pied(p: &egui::Painter, r: Rect) {
    p.rect_filled(r, 6.0, erp::FILET_PALE);
    p.hline(r.x_range(), r.top() + 0.5, Stroke::new(1.0, erp::FILET_BOUTON));
}

/// Le pied du tableau, peint à sa place `r` (collé au bas de l'écran tant que les rangées continuent dessous) : sous
/// chaque colonne de montants, sa somme (la note de crédit retranchée) ; `entier`, aussi le compte à gauche et le
/// reste à droite (sinon, le tableau défilant de côté, ils sont épinglés dessous : [`epingle`]).
fn pied(p: &egui::Painter, r: Rect, cols: &[(Colonne, f32)], t: &Totaux, cote: Cote, entier: bool) {
    let (gauche, droite) = if entier { (compte_du_pied(t), reste_du_pied(t, cote)) } else { (Vec::new(), Vec::new()) };
    fond_du_pied(p, r);
    let haut = r.top() + 8.0;
    // Où commencent les montants, et la colonne du reste (ou du paiement).
    let mut x = r.left();
    let mut debut_des_montants = None;
    let mut debut_du_reste = None;
    let s = &t.comptees;
    for (col, l) in cols {
        if col.en_montant() && *col != Colonne::Reste && debut_des_montants.is_none() {
            debut_des_montants = Some(x);
        }
        if matches!(col, Colonne::Reste | Colonne::Paiement) {
            debut_du_reste = Some(x);
        }
        let somme = match col {
            Colonne::AvantTaxes => Some(s.sous_total),
            Colonne::Tps => Some(s.tps),
            Colonne::Tvq => Some(s.tvq),
            Colonne::Taxes => Some(s.tps + s.tvq),
            Colonne::Total => Some(s.total),
            _ => None,
        };
        if let Some(m) = somme {
            let g = p.layout_no_wrap(argent(m), semi(TAILLE), erp::TITRE);
            let (lg, h) = (g.size().x, g.size().y);
            p.galley(pos2(x + l - MARGE - lg, haut + (19.0 - h) / 2.0), g, erp::TITRE);
        }
        x += l;
    }
    let place_a_gauche = debut_des_montants.unwrap_or(r.right()) - r.left() - 2.0 * MARGE;
    peindre_le_compte(p, pos2(r.left() + MARGE, haut), gauche, place_a_gauche);
    let x_reste = debut_du_reste.unwrap_or(r.right() - 300.0) + MARGE;
    peindre_le_reste(p, pos2(x_reste, haut), droite, r.right() - x_reste - MARGE);
}

/// Le compte du pied, une ligne de 19 points chacune depuis `coin` : « Total : N factures », puis ce qui est à part.
fn peindre_le_compte(p: &egui::Painter, coin: Pos2, lignes: Vec<(String, bool)>, largeur: f32) {
    for (k, (texte, fort)) in lignes.into_iter().enumerate() {
        let (police, encre) = if fort { (semi(TAILLE), erp::TITRE) } else { (normale(12.5), erp::TEXTE_2) };
        let g = ligne(p, texte, police, encre, largeur);
        let h = g.size().y;
        p.galley(pos2(coin.x, coin.y + 19.0 * k as f32 + (19.0 - h) / 2.0), g, encre);
    }
}

/// Le reste du pied, une ligne de 19 points chacune depuis `coin` : l'encaissé, le reste, ce qui en est en retard.
fn peindre_le_reste(p: &egui::Painter, coin: Pos2, lignes: Vec<(String, Color32)>, largeur: f32) {
    for (k, (texte, encre)) in lignes.into_iter().enumerate() {
        let police = if k == 0 || encre == erp::TITRE { semi(TAILLE) } else { normale(12.5) };
        let g = ligne(p, texte, police, encre, largeur);
        let h = g.size().y;
        p.galley(pos2(coin.x, coin.y + 19.0 * k as f32 + (19.0 - h) / 2.0), g, encre);
    }
}

/// Sous un tableau qui défile de côté, hors de ce qui défile : le compte à gauche, le reste à droite, collés au bas de
/// l'écran tant que le tableau continue dessous — jamais cachés par le défilement de côté. `haut` : le haut du
/// tableau (la rangée ne remonte pas au-dessus).
fn epingle(ui: &mut egui::Ui, t: &Totaux, cote: Cote, hauteur: f32, haut: f32) {
    let (place, _) = ui.allocate_exact_size(vec2(ui.available_width(), hauteur), Sense::hover());
    let ecran = ui.clip_rect();
    let y = place.top().min(ecran.bottom() - hauteur).max(haut);
    let r = Rect::from_min_size(pos2(place.left(), y), place.size());
    let p = ui.painter();
    p.rect_filled(r, 6.0, erp::FILET_PALE);
    let moitie = r.width() / 2.0;
    peindre_le_compte(p, pos2(r.left() + MARGE, r.top() + 8.0), compte_du_pied(t), moitie - 2.0 * MARGE);
    peindre_le_reste(p, pos2(r.center().x + MARGE, r.top() + 8.0), reste_du_pied(t, cote), moitie - 2.0 * MARGE);
}

// ---- Les cartes de la fenêtre étroite -------------------------------------------------------------------------------

/// Étroite : une carte par facture, dessinées seulement quand elles se voient (la page seule défile), puis les totaux,
/// collés au bas de l'écran tant que les cartes continuent dessous, comme la barre des totaux d'EG au téléphone.
fn cartes(ui: &mut egui::Ui, c: &Contexte<'_>, l: &Liste, cote: Cote) -> Option<(Choix, usize)> {
    let n_lignes = l.lignes.len();
    let n = n_lignes + if l.annulees.is_empty() { 0 } else { 1 + l.annulees.len() };
    let largeur = ui.available_width();
    let p = ui.painter().clone();
    let taxes = noms_des_taxes(ui.ctx(), c);
    let totaux = mise_en_page_des_totaux(&p, &l.totaux, cote, &taxes, largeur);
    let corps = n as f32 * CARTE;
    let (zone, _) = ui.allocate_exact_size(vec2(largeur, corps + 4.0 + totaux.hauteur), Sense::hover());
    let ecran = ui.clip_rect();
    let naturel = zone.top() + corps + 4.0;
    let y_totaux = naturel.min(ecran.bottom() - totaux.hauteur).max(zone.top());
    // Collés, ils touchent le bord de l'écran, coins du bas carrés : rien ne se voit dessous.
    let colles = y_totaux < naturel - 0.5;
    let mut choix = None;
    let premiere = (((ecran.top() - zone.top()) / CARTE).floor().max(0.0) as usize).min(n);
    let derniere = (((ecran.bottom() - zone.top()) / CARTE).ceil().max(0.0) as usize).min(n);
    for k in premiere..derniere {
        let r = Rect::from_min_size(pos2(zone.left(), zone.top() + k as f32 * CARTE), vec2(largeur, CARTE));
        let carte = if k < n_lignes {
            Some((l.lignes[k], false))
        } else if k == n_lignes {
            titre_des_annulees(&p, Rect::from_min_size(r.min, vec2(largeur, RANGEE)), l.annulees.len());
            None
        } else {
            l.annulees.get(k - n_lignes - 1).map(|i| (*i, true))
        };
        // Sous les totaux collés, une carte ne répond pas à la souris.
        let visible = Rect::from_min_max(r.min, pos2(r.right(), r.bottom().min(y_totaux)));
        if let Some((i, annulee)) = carte
            && let Some(x) = carte_d_une_facture(ui, c, i, annulee, cote, r, visible)
        {
            choix = Some(x);
        }
    }
    peindre_les_totaux(&p, pos2(zone.left(), y_totaux), largeur, totaux, colles);
    choix
}

/// Les badges d'une carte : son statut (un brouillon, une annulée, une autre sorte), sa fiabilité à vérifier, son
/// paiement ou son reste.
fn badges(f: &Facture, aujourdhui: &str) -> Vec<(String, erp::Teinte)> {
    let mut b = Vec::new();
    if f.annulee() || f.brouillon() || calcul::sorte_dite(f).is_some() {
        b.push((calcul::statut_dit(f), if f.annulee() || f.brouillon() { erp::GRIS } else { erp::INDIGO }));
    }
    if f.cote == Cote::Fournisseur && Fiabilite::de(f) == Fiabilite::AVerifier {
        b.push((Fiabilite::AVerifier.mot().to_string(), erp::ORANGE));
    }
    match f.cote {
        Cote::Fournisseur => match calcul::reste_du_fournisseur(f, aujourdhui) {
            Reste::APayer(c, 0) => b.push((format!("À payer {}", argent(c)), erp::JAUNE)),
            Reste::APayer(c, j) => b.push((format!("En retard · {j} j · {}", argent(c)), erp::ROUGE)),
            Reste::Payee(d) => b.push((d.map_or_else(|| "Payée".into(), |d| format!("Payée le {d}")), erp::VERT)),
            Reste::Rien => {}
        },
        Cote::Client => match calcul::paiement_du_client(f, aujourdhui) {
            Paiement::Encaissee(d) => {
                b.push((d.map_or_else(|| "Encaissée".into(), |d| format!("Encaissée le {d}")), erp::VERT))
            }
            Paiement::ARecevoir(c) => b.push((format!("À recevoir {}", argent(c)), erp::JAUNE)),
            Paiement::EnRetard(j, c) => b.push((format!("En retard · {j} j · {}", argent(c)), erp::ROUGE)),
            Paiement::Credit => b.push(("Crédit".into(), erp::BLEU)),
            Paiement::Soldee => b.push(("Soldée".into(), erp::VERT)),
            Paiement::MarqueePayee => b.push(("Marquée payée".into(), erp::VERT)),
            Paiement::Rien => {}
        },
    }
    b
}

/// Une carte, à sa place `place` (son écart compris) : la date et le total ; le tiers et le numéro ; le chantier et la
/// section ; ses badges. Un clic, son menu — dans `visible` seulement (pas sous les totaux collés).
#[allow(clippy::too_many_arguments)]
fn carte_d_une_facture(
    ui: &mut egui::Ui,
    c: &Contexte<'_>,
    i: usize,
    annulee: bool,
    cote: Cote,
    place: Rect,
    visible: Rect,
) -> Option<(Choix, usize)> {
    let f = c.registre.factures.get(i)?;
    let r = Rect::from_min_max(place.min, pos2(place.right(), place.bottom() - ECART_DES_CARTES));
    let touchable = r.intersect(visible);
    let sens = if touchable.height() >= r.height() / 2.0 { Sense::click() } else { Sense::hover() };
    let touchable = if touchable.height() > 0.0 { touchable } else { Rect::NOTHING };
    let rep = ui.interact(touchable, egui::Id::unique(("compta-liste-carte", cote, f.id)), sens);
    let ouvert = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&rep));
    let p = ui.painter().clone();
    let fond = if annulee {
        erp::FILET_PALE
    } else if rep.hovered() || ouvert {
        erp::SURVOL
    } else {
        Color32::WHITE
    };
    p.rect(r, 10.0, fond, Stroke::new(1.0, erp::FILET_DE_CARTE), StrokeKind::Inside);
    let (gauche, large) = (r.left() + 14.0, r.width() - 28.0);
    let pale = |x: Color32| if annulee { erp::TEXTE_3 } else { x };
    // La date et le total.
    let total = if f.total.is_some() { argent(f.total_signe()) } else { "\u{2014}".into() };
    let g_total = p.layout_no_wrap(total, semi(15.0), pale(erp::TITRE));
    let (l_total, h_total) = (g_total.size().x, g_total.size().y);
    let y = r.top() + 10.0;
    p.galley(pos2(r.right() - 14.0 - l_total, y), g_total, pale(erp::TITRE));
    if annulee {
        p.hline(r.right() - 14.0 - l_total..=r.right() - 14.0, y + h_total / 2.0, Stroke::new(1.0, erp::TEXTE_3));
    }
    let date = f.date.clone().unwrap_or_else(|| "sans date".to_string());
    let g = ligne(&p, date, normale(12.5), pale(erp::TEXTE_2), large - l_total - 12.0);
    p.galley(pos2(gauche, y + 2.0), g, pale(erp::TEXTE_2));
    // Le tiers et le numéro (le numéro d'abord pour un client, comme EG).
    let numero = calcul::numero_dit(f);
    let numero = if numero.is_empty() { "sans numéro".to_string() } else { format!("n° {numero}") };
    let tiers = f.tiers.trim();
    let titre = match (cote, tiers.is_empty()) {
        (_, true) => numero,
        (Cote::Fournisseur, false) => format!("{tiers} · {numero}"),
        (Cote::Client, false) => format!("{numero} · {tiers}"),
    };
    let g = ligne(&p, titre, semi(14.5), pale(erp::TITRE), large);
    p.galley(pos2(gauche, r.top() + 32.0), g, pale(erp::TITRE));
    // Le chantier et la section.
    let l2: Vec<&str> = [f.projet.as_deref(), f.section.as_deref()]
        .into_iter()
        .flatten()
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .collect();
    if !l2.is_empty() {
        let g = ligne(&p, l2.join(" · "), normale(12.5), pale(erp::TEXTE_2), large);
        p.galley(pos2(gauche, r.top() + 53.0), g, pale(erp::TEXTE_2));
    }
    // Les badges.
    let mut x = gauche;
    for (texte, teinte) in badges(f, c.aujourdhui) {
        let reste = r.right() - 14.0 - x;
        if reste < 40.0 {
            break;
        }
        let teinte = if annulee { erp::GRIS } else { teinte };
        let g = ligne(&p, texte, semi(11.5), teinte.encre, reste - 18.0);
        let taille = g.size() + vec2(18.0, 7.0);
        let place = Rect::from_min_size(pos2(x, r.top() + 72.0), taille);
        theme::peindre_badge(&p, place, g, teinte);
        x += taille.x + 6.0;
    }
    let rep = rep.on_hover_cursor(egui::CursorIcon::PointingHand);
    menu_de_la_facture(&rep, cote, f.id).map(|x| (x, i))
}

/// Les lignes des totaux en bas des cartes, sous leur première (le nombre et le total) : l'avant-taxes et les taxes ;
/// l'encaissé et le reste à recevoir, ou le reste à payer ; ce qui ne compte pas. À la française : un montant ne se
/// coupe jamais en fin de ligne (egui coupe à l'espace fine des milliers, pas à l'insécable : [`insecables`]).
///
/// [`insecables`]: super::super::insecables
///
/// Les taxes sous leurs noms (`taxes`, ceux des en-têtes du tableau) ; une seconde absente ne se dit pas : sous le nom
/// de la seule, la somme des deux cases, comme les tuiles du tableau de bord (la QA5, passe 2 : le pied et la tuile
/// divergeaient sur une seconde case montée sans nom).
fn lignes_des_totaux(t: &Totaux, cote: Cote, taxes: &(String, String)) -> Vec<(String, Color32)> {
    let s = &t.comptees;
    let premiere = if taxes.1.is_empty() { s.tps + s.tvq } else { s.tps };
    let mut avant = format!("Avant taxes {} · {} {}", argent(s.sous_total), taxes.0, argent(premiere));
    if !taxes.1.is_empty() {
        avant.push_str(&format!(" · {} {}", taxes.1, argent(s.tvq)));
    }
    let mut lignes = vec![(avant, erp::TEXTE_2)];
    let reste = reste_du_pied(t, cote);
    lignes.push((reste.iter().map(|(x, _)| x.as_str()).collect::<Vec<_>>().join(" · "), erp::TEXTE));
    for (x, _) in compte_du_pied(t).into_iter().skip(1) {
        lignes.push((x, erp::TEXTE_2));
    }
    lignes.into_iter().map(|(x, encre)| (super::super::insecables(&x), encre)).collect()
}

/// Les totaux en bas des cartes, mis en page à `largeur` : leur première ligne (le nombre, le total), les autres, et
/// la hauteur qu'il leur faut.
struct TotauxEnBas {
    nombre: Arc<egui::Galley>,
    total: Arc<egui::Galley>,
    lignes: Vec<(Arc<egui::Galley>, Color32)>,
    hauteur: f32,
}

const MARGE_DES_TOTAUX: f32 = 16.0;

fn mise_en_page_des_totaux(
    p: &egui::Painter,
    t: &Totaux,
    cote: Cote,
    taxes: &(String, String),
    largeur: f32,
) -> TotauxEnBas {
    let s = &t.comptees;
    let nombre = p.layout_no_wrap(factures(s.n), semi(14.5), erp::TITRE);
    let total = p.layout_no_wrap(super::super::insecables(&argent(s.total)), semi(15.0), erp::TITRE);
    let dedans = (largeur - 2.0 * MARGE_DES_TOTAUX).max(40.0);
    let lignes: Vec<(Arc<egui::Galley>, Color32)> = lignes_des_totaux(t, cote, taxes)
        .into_iter()
        .map(|(x, encre)| (p.layout(x, normale(12.5), encre, dedans), encre))
        .collect();
    let hauteur =
        2.0 * 14.0 + nombre.size().y.max(total.size().y) + lignes.iter().map(|(g, _)| g.size().y + 4.0).sum::<f32>();
    TotauxEnBas { nombre, total, lignes, hauteur }
}

/// Les totaux en bas des cartes, peints depuis `coin` : une carte blanche, son ombre (des cartes passent dessous) ;
/// `colles` au bord de l'écran, ses coins du bas carrés (rien ne se voit dessous).
fn peindre_les_totaux(p: &egui::Painter, coin: Pos2, largeur: f32, t: TotauxEnBas, colles: bool) {
    let r = Rect::from_min_size(coin, vec2(largeur, t.hauteur));
    let coins = if colles { egui::CornerRadius { nw: 12, ne: 12, sw: 0, se: 0 } } else { egui::CornerRadius::same(12) };
    p.add(theme::ombre_marine(r, 12, -2, 16, 40));
    p.rect(r, coins, Color32::WHITE, Stroke::new(1.0, erp::FILET_DE_CARTE), StrokeKind::Inside);
    let (gauche, droite) = (r.left() + MARGE_DES_TOTAUX, r.right() - MARGE_DES_TOTAUX);
    let mut y = r.top() + 14.0;
    let h = t.nombre.size().y.max(t.total.size().y);
    let l_total = t.total.size().x;
    p.galley(pos2(gauche, y + (h - t.nombre.size().y) / 2.0), t.nombre, erp::TITRE);
    p.galley(pos2(droite - l_total, y + (h - t.total.size().y) / 2.0), t.total, erp::TITRE);
    y += h + 4.0;
    for (g, encre) in t.lignes {
        let hg = g.size().y;
        p.galley(pos2(gauche, y), g, encre);
        y += hg + 4.0;
    }
}

#[cfg(test)]
mod essais {
    use super::*;
    use crate::compta::Somme;

    /// Des totaux d'essai : quatre revenus, un brouillon et une pro forma montrés, une annulée, une sans total.
    fn totaux() -> Totaux {
        Totaux {
            comptees: Somme {
                n: 4,
                sous_total: 3_400_000,
                tps: 170_000,
                tvq: 339_150,
                total: 3_909_150,
                sans_total: 1,
            },
            non_comptees: Somme { n: 2, total: 2_173_028, ..Somme::default() },
            brouillons_non_comptes: 1,
            pro_forma: 1,
            annulees: 1,
            reste: 2_529_450,
            n_reste: 2,
            en_retard: 1_149_750,
            n_en_retard: 1,
            encaisse: 1_494_675,
        }
    }

    #[test]
    fn le_pied_dit_ce_qui_compte_et_ce_qui_reste() {
        let t = totaux();
        let gauche: Vec<String> = compte_du_pied(&t).into_iter().map(|(x, _)| x).collect();
        assert_eq!(
            gauche,
            [
                "Total : 4 factures",
                "+ 2 non comptées (1 brouillon, 1 pro forma) : 21\u{202f}730,28\u{a0}$",
                "+ 1 annulée, non comptée",
                "1 sans total",
            ]
        );
        let droite: Vec<String> = reste_du_pied(&t, Cote::Client).into_iter().map(|(x, _)| x).collect();
        assert_eq!(
            droite,
            [
                "Encaissé 14\u{202f}946,75\u{a0}$",
                "À recevoir 25\u{202f}294,50\u{a0}$",
                "dont 1 en retard : 11\u{202f}497,50\u{a0}$",
            ]
        );
        let droite: Vec<String> = reste_du_pied(&t, Cote::Fournisseur).into_iter().map(|(x, _)| x).collect();
        assert_eq!(droite, ["Reste à payer 25\u{202f}294,50\u{a0}$", "dont 1 en retard : 11\u{202f}497,50\u{a0}$"]);
        // La QA5 (passe 2) : les comptes séparent leurs milliers, comme l'argent (« Total : 13566 factures »).
        let mut grands = totaux();
        (grands.comptees.n, grands.n_en_retard, grands.annulees) = (13_566, 2_861, 1_204);
        let gauche: Vec<String> = compte_du_pied(&grands).into_iter().map(|(x, _)| x).collect();
        assert_eq!(
            (gauche[0].as_str(), gauche[2].as_str()),
            ("Total : 13\u{202f}566 factures", "+ 1\u{202f}204 annulées, non comptées")
        );
        let droite: Vec<String> = reste_du_pied(&grands, Cote::Client).into_iter().map(|(x, _)| x).collect();
        assert_eq!(droite[2], "dont 2\u{202f}861 en retard : 11\u{202f}497,50\u{a0}$");
        assert_eq!(titre_d_annulees(1_204), "1\u{202f}204 factures annulées — montrées à part, jamais comptées");
        // Le titre des annulées s'accorde.
        assert_eq!(titre_d_annulees(1), "1 facture annulée — montrée à part, jamais comptée");
        assert_eq!(titre_d_annulees(3), "3 factures annulées — montrées à part, jamais comptées");
        // Le pied prend la hauteur de sa plus longue colonne : quatre lignes à gauche.
        assert_eq!(hauteur_du_pied(&t, Cote::Client), 16.0 + 19.0 * 4.0);
        // Sans rien à part : une seule ligne, « 1 facture » au singulier.
        let seule = Totaux { comptees: Somme { n: 1, ..Somme::default() }, ..Totaux::default() };
        assert_eq!(compte_du_pied(&seule), [("Total : 1 facture".to_string(), true)]);
        assert_eq!(
            non_comptees(&Totaux { pro_forma: 3, non_comptees: Somme { n: 3, ..Somme::default() }, ..seule })
                .as_deref(),
            Some("+ 3 non comptées (3 pro forma) : 0,00\u{a0}$")
        );
    }

    /// Les colonnes d'une disposition, et leur largeur.
    fn posees(cote: Cote, dispo: f32, pied: f32) -> Vec<(Colonne, f32)> {
        disposer(cote, |c| if c == Colonne::AvantTaxes { pied } else { 0.0 }, dispo)
    }

    #[test]
    fn les_colonnes_fondent_puis_se_resserrent_quand_la_place_manque() {
        use Colonne::*;
        let f = Cote::Fournisseur;
        // À l'aise (1 542 points) : les douze, les 90 points de plus aux textes, selon leur part (3, 1, 2, 2 et 1).
        assert_eq!(
            posees(f, 1_632.0, 0.0),
            [
                (Date, 96.0),
                (Tiers, 200.0),
                (Numero, 140.0),
                (Chantier, 170.0),
                (Section, 180.0),
                (AvantTaxes, 112.0),
                (Tps, 92.0),
                (Tvq, 96.0),
                (Total, 120.0),
                (Reste, 156.0),
                (Fiabilite, 100.0),
                (Statut, 170.0),
            ]
        );
        // La place manque : la TPS et la TVQ fondues en « Taxes » (1 466 points), comme chez les clients.
        assert_eq!(
            posees(f, 1_484.0, 0.0),
            [
                (Date, 96.0),
                (Tiers, 176.0),
                (Numero, 132.0),
                (Chantier, 154.0),
                (Section, 164.0),
                (AvantTaxes, 112.0),
                (Taxes, 112.0),
                (Total, 120.0),
                (Reste, 156.0),
                (Fiabilite, 100.0),
                (Statut, 162.0),
            ]
        );
        // Encore moins : chacune cède la moitié de ce qu'elle peut (176 points manquent sur 352), et tout tient.
        let serrees = posees(f, 1_290.0, 0.0);
        assert_eq!(
            serrees,
            [
                (Date, 93.0),
                (Tiers, 137.0),
                (Numero, 107.0),
                (Chantier, 121.0),
                (Section, 122.0),
                (AvantTaxes, 108.0),
                (Taxes, 108.0),
                (Total, 115.0),
                (Reste, 151.0),
                (Fiabilite, 98.0),
                (Statut, 130.0),
            ]
        );
        assert_eq!(serrees.iter().map(|c| c.1).sum::<f32>(), 1_290.0);
        // Trop peu : toutes au plus serré (1 114 points) ; le tableau défilera de côté. Un montant ne descend jamais
        // sous sa somme au pied.
        let trop = posees(f, 1_000.0, 130.0);
        assert_eq!(trop.iter().map(|c| c.1).sum::<f32>(), 1_114.0 - 104.0 + 130.0);
        assert_eq!(trop[5], (AvantTaxes, 130.0));
        // Les clients (1 340 points à l'aise) n'ont rien à fondre : ils se resserrent.
        assert_eq!(posees(Cote::Client, 1_410.0, 0.0)[2], (Tiers, 220.0));
        assert_eq!(
            posees(Cote::Client, 1_152.0, 0.0),
            [
                (Date, 93.0),
                (Numero, 117.0),
                (Tiers, 147.0),
                (Chantier, 131.0),
                (AvantTaxes, 108.0),
                (Taxes, 108.0),
                (Total, 115.0),
                (Paiement, 188.0),
                (Statut, 145.0),
            ]
        );
    }

    #[test]
    fn l_en_tete_et_le_pied_restent_en_vue() {
        // Un tableau en haut d'une page de 600 points : l'en-tête à sa place, le pied collé au bas de l'écran.
        assert_eq!(collants(100.0, 34.0, 1_000.0, 54.0, 0.0, (0.0, 600.0)), (100.0, 546.0));
        // Défilé : l'en-tête collé au haut de l'écran, le pied au bas.
        assert_eq!(collants(100.0, 34.0, 1_000.0, 54.0, 0.0, (500.0, 1_100.0)), (500.0, 1_046.0));
        // Sa fin à l'écran : le pied à sa place, sous la dernière rangée.
        assert_eq!(collants(100.0, 34.0, 1_000.0, 54.0, 0.0, (900.0, 1_500.0)), (900.0, 1_134.0));
        // Défilé de côté : le compte et le reste épinglés dessous (60 points) ; les sommes, au-dessus d'eux.
        assert_eq!(collants(100.0, 34.0, 1_000.0, 35.0, 60.0, (0.0, 600.0)).1, 505.0);
        // Pas encore à l'écran : rien ne remonte au-dessus de son tableau.
        assert_eq!(collants(2_000.0, 34.0, 1_000.0, 54.0, 0.0, (0.0, 600.0)), (2_000.0, 2_034.0));
    }

    #[test]
    fn les_totaux_en_bas_ne_coupent_pas_un_montant() {
        // La QA5 (passe 1) : « TVQ 6 » puis « 754,79 $ » à la ligne : egui coupait à l'espace fine des milliers.
        let t = Totaux {
            comptees: Somme {
                n: 10,
                sous_total: 6_771_715,
                tps: 338_587,
                tvq: 675_479,
                total: 7_785_781,
                sans_total: 0,
            },
            ..Totaux::default()
        };
        let taxes = ("TPS".to_string(), "TVQ".to_string());
        let lignes: Vec<String> = lignes_des_totaux(&t, Cote::Fournisseur, &taxes).into_iter().map(|x| x.0).collect();
        assert_eq!(
            lignes[0],
            "Avant taxes 67\u{a0}717,15\u{a0}$ · TPS 3\u{a0}385,87\u{a0}$ · TVQ 6\u{a0}754,79\u{a0}$"
        );
        assert!(lignes.iter().all(|x| !x.contains('\u{202f}')), "{lignes:?}");
        // Une seule taxe (une TVH) : la seconde ne se dit pas ; sous le nom de la seule, la somme des deux cases,
        // comme la tuile du tableau de bord (3 385,87 + 6 754,79 = 10 140,66 $).
        let tvh = ("TVH".to_string(), String::new());
        let lignes = lignes_des_totaux(&t, Cote::Fournisseur, &tvh);
        assert_eq!(lignes[0].0, "Avant taxes 67\u{a0}717,15\u{a0}$ · TVH 10\u{a0}140,66\u{a0}$");
    }
}

/// QA5 du lot 13.2, passe 2 (mutation) : les essais qui manquaient à la disposition des listes.
#[cfg(test)]
mod manques_qa2 {
    use super::*;

    /// Le tableau a défilé plus bas que sa dernière rangée : l'en-tête collant s'arrête au bas de son tableau, jamais
    /// sous lui (mutant 719).
    #[test]
    fn manque_719_l_en_tete_s_arrete_au_bas_de_son_tableau() {
        assert_eq!(collants(100.0, 34.0, 1_000.0, 54.0, 0.0, (1_200.0, 1_800.0)).0, 1_100.0);
    }

    /// Juste à l'aise (1 542 points pour les fournisseurs) : les douze colonnes, la TPS et la TVQ séparées (mutant 711).
    #[test]
    fn manque_711_juste_a_l_aise_les_douze_colonnes() {
        let posees = disposer(Cote::Fournisseur, |_| 0.0, 1_542.0);
        assert_eq!(posees.len(), 12);
        assert!(posees.iter().any(|c| c.0 == Colonne::Tvq));
    }

    /// À l'aise, un montant n'est jamais plus étroit que sa somme au pied (mutant 715).
    #[test]
    fn manque_715_a_l_aise_un_montant_garde_la_largeur_de_sa_somme() {
        let posees = disposer(Cote::Fournisseur, |c| if c == Colonne::AvantTaxes { 130.0 } else { 0.0 }, 2_000.0);
        assert_eq!(posees.iter().find(|c| c.0 == Colonne::AvantTaxes).map(|c| c.1), Some(130.0));
    }
}
