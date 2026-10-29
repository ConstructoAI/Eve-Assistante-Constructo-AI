//! Le planning dans le tableau de bord du thème « Constructo AI » (demandé le 28 octobre : « un joli calendrier, Gantt
//! et Kanban dans l'interface ») : des onglets en tête du tableau de bord — « Tableau de bord · Calendrier · Gantt ·
//! Kanban », celui d'aujourd'hui restant le premier ([`super::tableau`]) —, et les trois vues du planning de
//! [`crate::planning`], que donne le centre ([`crate::centre::Centre::planning`]) : l'entreprise, lue dans Constructo
//! AI en lecture seule, et la maison. Le calendrier ([`calendrier`]) : un mois ou une semaine, chaque chose dans la
//! teinte de sa sorte ; le Gantt ([`gantt`]) : les chantiers et leurs opérations dans le temps ; le Kanban ([`kanban`])
//! : un tableau du suivi de l'ERP au choix, ses colonnes et leurs cartes. Le choix « Tout · Entreprise · Famille » vaut
//! pour les quatre ; sans Constructo AI (pas permis à cette Eve), ni Gantt ni Kanban : ils ne montrent que
//! l'entreprise.
//!
//! Toucher une chose — un événement, une tâche, une carte — ouvre son menu, comme une rangée du tableau de bord : « En
//! parler à Eve », « Me le rappeler demain », et pour une tâche « Replanifier… », pour une carte « Déplacer vers… ». Ce
//! sont des demandes à Eve ([`Action`]), jamais des écritures : c'est Claude qui écrit dans l'ERP, et l'accord de la
//! personne vaut là où les accords valent. Chaque demande porte la `source` de la chose, ce qu'Eve en dit à Claude.
//!
//! Ce que la fenêtre retient de ces vues — l'onglet ouvert, le mois ou la semaine montrés, l'échelle du Gantt et ses
//! chantiers dépliés, le tableau du Kanban — vit dans la mémoire d'egui le temps que la fenêtre vit : rien ne s'écrit.

mod calendrier;
mod gantt;
mod kanban;

use std::sync::{Arc, Mutex, PoisonError};

use eframe::egui::{self, Align2, Color32, FontFamily, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, pos2, vec2};
use serde_json::Value;

use super::tableau::{Liaison, Vue};
use super::theme::{self, BoutonErp, erp, pictos};
use super::{Fenetre, GRAS, ICONES, SEMI};
use crate::centre;
use crate::planning::{Carte, Cote, Evenement, Planning, Sorte, Tache};

// ---- Les onglets ----------------------------------------------------------------------------------------------------

/// Les onglets en tête du tableau de bord : le tableau de bord d'aujourd'hui, puis les trois vues du planning.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Onglet {
    Tableau,
    Calendrier,
    Gantt,
    Kanban,
}

/// Chaque onglet : son icône (Segoe Fluent Icons, vérifiées sur une planche rendue — les quatre tuiles du tableau de
/// bord, le calendrier, les barres décalées d'un Gantt, les colonnes d'un tableau), son nom, sa bulle.
const ONGLETS: [(Onglet, char, &str, &str); 4] = [
    (Onglet::Tableau, '\u{F0E2}', "Tableau de bord", "Ce qui presse, la journée, l'entreprise et la famille"),
    (Onglet::Calendrier, '\u{E787}', "Calendrier", "Le mois ou la semaine : l'entreprise et la maison"),
    (Onglet::Gantt, '\u{EABB}', "Gantt", "Les chantiers et leurs opérations dans le temps"),
    (Onglet::Kanban, '\u{E8C0}', "Kanban", "Les tableaux du suivi de Constructo AI, colonne par colonne"),
];

/// Où la fenêtre garde l'onglet ouvert.
const ONGLET: &str = "onglet-du-tableau-de-bord";

impl Onglet {
    /// Le Gantt et le Kanban ne montrent que l'entreprise.
    fn de_l_entreprise(self) -> bool {
        matches!(self, Onglet::Gantt | Onglet::Kanban)
    }
}

/// L'onglet ouvert : le tableau de bord d'office.
pub(crate) fn onglet(ctx: &egui::Context) -> Onglet {
    ctx.data(|d| d.get_temp::<Onglet>(egui::Id::unique(ONGLET))).unwrap_or(Onglet::Tableau)
}

/// Ouvre un onglet (un clic, ou une scène de l'essai du thème).
pub(crate) fn choisir_l_onglet(ctx: &egui::Context, o: Onglet) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::unique(ONGLET), o));
    ctx.request_repaint();
}

/// La rangée des onglets : chacun son icône et son nom ; l'ouvert à l'encre des titres, son icône de l'accent, souligné
/// d'un trait du dégradé de la bannière ; au survol, un voile blanc ; un filet marine très pâle sous la rangée (le fond
/// qui respire le porte). Sans l'entreprise, ni Gantt ni Kanban. Rend l'onglet cliqué.
fn onglets(ui: &mut egui::Ui, ouvert: Onglet, avec_l_entreprise: bool) -> Option<Onglet> {
    let (rangee, _) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::hover());
    let p = ui.painter().clone();
    p.hline(rangee.x_range(), rangee.bottom() - 0.5, Stroke::new(1.0, erp::MARINE_HAUT.gamma_multiply(0.16)));
    let police = FontId::new(15.0, FontFamily::Name(SEMI.into()));
    let icones = FontId::new(16.0, FontFamily::Name(ICONES.into()));
    let aucune = Color32::PLACEHOLDER;
    let mut x = rangee.left();
    let mut clic = None;
    for (k, (o, icone, mot, aide)) in ONGLETS.iter().enumerate() {
        if o.de_l_entreprise() && !avec_l_entreprise {
            continue;
        }
        let g_icone = p.layout_no_wrap(icone.to_string(), icones.clone(), aucune);
        let g_mot = p.layout_no_wrap(mot.to_string(), police.clone(), aucune);
        let l = 14.0 + g_icone.size().x + 9.0 + g_mot.size().x + 14.0;
        let r = Rect::from_min_size(pos2(x, rangee.top()), vec2(l, rangee.height() - 4.0));
        x += l + 4.0;
        let rep = ui.interact(r, egui::Id::unique(("onglet-du-tableau", k)), Sense::click());
        let choisi = *o == ouvert;
        if rep.hovered() && !choisi {
            p.rect_filled(r.shrink2(vec2(0.0, 3.0)), 10.0, Color32::from_white_alpha(150));
        }
        let (encre, accent) = match (choisi, rep.hovered()) {
            (true, _) => (erp::TITRE, erp::PRIMAIRE),
            (false, true) => (erp::TEXTE, erp::TEXTE),
            _ => (erp::TEXTE_2, erp::TEXTE_2),
        };
        let l_icone = g_icone.size().x;
        p.galley(pos2(r.left() + 14.0, r.center().y - g_icone.size().y / 2.0), g_icone, accent);
        p.galley(pos2(r.left() + 14.0 + l_icone + 9.0, r.center().y - g_mot.size().y / 2.0), g_mot, encre);
        if choisi {
            let soulignement = Rect::from_min_max(
                pos2(r.left() + 10.0, rangee.bottom() - 3.0),
                pos2(r.right() - 10.0, rangee.bottom()),
            );
            let arrets = [erp::MARINE_HAUT, erp::AZUR];
            p.add(theme::rectangle_degrade(soulignement, 1.5, |q| theme::degrade_css(soulignement, 90.0, &arrets, q)));
        }
        if rep.on_hover_text(*aide).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() && !choisi {
            clic = Some(*o);
        }
    }
    clic
}

// ---- Le planning du moment --------------------------------------------------------------------------------------

/// Un planning imposé : la vitrine de `eve --essai-theme` (une semaine inventée d'une entreprise et d'une famille).
/// Tant qu'il l'est, les vues le montrent à la place de celui du centre. Eve ne l'impose jamais.
static IMPOSE: Mutex<Option<Planning>> = Mutex::new(None);

/// Impose ce planning aux vues (la vitrine de l'essai du thème), ou rend la main au centre (`None`).
pub(crate) fn imposer(p: Option<Planning>) {
    *IMPOSE.lock().unwrap_or_else(PoisonError::into_inner) = p;
}

fn impose() -> Option<Planning> {
    IMPOSE.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

/// Après tant de secondes sans se montrer, une vue qui demande les opérations les redemande en se montrant.
const DE_NOUVEAU: f64 = 2.0;

impl Fenetre {
    /// Le planning à montrer : l'imposé (l'essai), sinon celui du centre — qui ne le refait que quand ce qu'il lit
    /// change : la fenêtre le demande à chaque image, et le lit par référence.
    fn planning_du_moment(&self, aujourdhui: &str) -> Arc<Planning> {
        match impose() {
            Some(p) => Arc::new(p),
            None => self.centre.planning(aujourdhui).into(),
        }
    }

    /// Les opérations du suivi de Constructo AI ne se lisent qu'à la demande (leur outil est lourd) : quand le Gantt se
    /// montre, ou le tableau des opérations du Kanban (`quoi`), la fenêtre le dit au chef — le geste `planning_lire`,
    /// une fois en se montrant ; le chef n'en relit pas plus d'une par demi-heure.
    fn lire_a_la_demande(&self, ctx: &egui::Context, quoi: &str) {
        let id = egui::Id::unique(("planning-montre", quoi));
        let maintenant = ctx.input(|i| i.time);
        let avant = ctx.data(|d| d.get_temp::<f64>(id));
        ctx.data_mut(|d| d.insert_temp(id, maintenant));
        if avant.is_none_or(|a| maintenant - a > DE_NOUVEAU) {
            self.geste("planning_lire", Value::Null);
        }
    }

    /// La rangée des onglets ([`onglets`]) ; rend l'onglet ouvert. Sans l'entreprise, un Gantt ou un Kanban ouvert
    /// revient au tableau de bord.
    pub(super) fn rangee_des_onglets(&mut self, ui: &mut egui::Ui, avec_l_entreprise: bool) -> Onglet {
        let mut ouvert = onglet(ui.ctx());
        if ouvert.de_l_entreprise() && !avec_l_entreprise {
            ouvert = Onglet::Tableau;
        }
        if let Some(o) = onglets(ui, ouvert, avec_l_entreprise) {
            choisir_l_onglet(ui.ctx(), o);
            ouvert = o;
        }
        ouvert
    }

    /// La page d'un onglet du planning, sous la rangée du choix de la vue : le calendrier (l'entreprise pas reliée, une
    /// invitation au-dessus de la maison), le Gantt ou le Kanban — l'entreprise seule : dans la vue de la famille, de
    /// quoi passer à l'entreprise ; pas reliée, l'invitation à se relier ; pas encore lue, la première lecture (ou ce
    /// qui l'a empêchée, `erreur`). L'état de la lecture est celui du tableau de bord (`entreprise_lue` : sa lecture de
    /// l'ERP a des chiffres), comme le planning lui-même s'il dit quand il a été lu. Ce qu'on y choisit part au chef
    /// ([`Fenetre::agir_sur_le_planning`]).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn page_du_planning(
        &mut self,
        ui: &mut egui::Ui,
        onglet: Onglet,
        vue: Vue,
        liaison: Liaison,
        entreprise_lue: bool,
        erreur: Option<String>,
        aujourdhui: &str,
    ) {
        let Some(jour) = centre::numero_du_jour(aujourdhui) else { return };
        let planning = self.planning_du_moment(aujourdhui);
        let planning: &Planning = &planning;
        let lue = entreprise_lue || !planning.lu.is_empty();
        let pas_reliee = matches!(liaison, Liaison::NonReliee | Liaison::EnCours);
        let action = match onglet {
            Onglet::Tableau => None,
            Onglet::Calendrier => {
                if pas_reliee && !lue {
                    if vue == Vue::Entreprise {
                        self.invitation(ui, liaison);
                        return;
                    }
                    let texte = "Relie Eve à Constructo AI : l'agenda de l'entreprise, ses chantiers, ses livraisons \
                                 et ses factures s'ajoutent à ce calendrier.";
                    if super::tableau::bandeau(ui, false, texte, ("Se connecter", pictos::LIEN)) {
                        self.geste("constructo_connecter", Value::Null);
                    }
                    ui.add_space(theme::ECART);
                }
                calendrier::vue(ui, planning, vue, jour)
            }
            Onglet::Gantt | Onglet::Kanban => {
                let (nom, icone) =
                    if onglet == Onglet::Gantt { ("Gantt", ONGLETS[2].1) } else { ("Kanban", ONGLETS[3].1) };
                if vue == Vue::Famille {
                    let texte = format!(
                        "Le {nom} suit l'entreprise, lue dans Constructo AI : choisis « Entreprise » ou « Tout » pour \
                         le voir."
                    );
                    if etat_vide(
                        ui,
                        icone,
                        erp::BLEU,
                        &format!("Le {nom} montre l'entreprise"),
                        &texte,
                        Some("Voir l'entreprise"),
                    ) {
                        self.choisir_la_vue(ui.ctx(), Vue::Entreprise);
                    }
                    return;
                }
                // Le Gantt montre les opérations : elles se demandent quand il se montre (l'entreprise reliée).
                if onglet == Onglet::Gantt && liaison == Liaison::Reliee {
                    self.lire_a_la_demande(ui.ctx(), "gantt");
                }
                if !lue {
                    match (pas_reliee, erreur) {
                        (true, _) => self.invitation(ui, liaison),
                        (false, Some(raison)) => self.carte_d_erreur(ui, &raison),
                        (false, None) => super::tableau::premiere_lecture(ui),
                    }
                    return;
                }
                if onglet == Onglet::Gantt {
                    let (action, tableau) = gantt::vue(ui, planning, jour);
                    // Ses données en tableau, comme les tableaux des cartes.
                    if let Some(json) = tableau {
                        theme::carte(ui, None, 0.0, |ui| self.cartes.tableau_seul(ui, &json));
                    }
                    action
                } else {
                    let (action, operations) = kanban::vue(ui, planning, jour);
                    if operations && liaison == Liaison::Reliee {
                        self.lire_a_la_demande(ui.ctx(), "operations");
                    }
                    action
                }
            }
        };
        if let Some(a) = action {
            self.agir_sur_le_planning(a);
        }
    }

    /// Ce qu'on a choisi dans une vue du planning : une demande à Eve, envoyée comme si on l'avait écrite (elle
    /// s'affiche dans la conversation) ; un rappel pour demain matin, que le chef pose sans Claude ; une demande à
    /// compléter, que la saisie commence (elle prend le clavier).
    fn agir_sur_le_planning(&mut self, a: Action) {
        match a {
            Action::Demander(t) => self.geste("texte", Value::String(t)),
            Action::Rappeler(t) => self.geste("rappel_demain", Value::String(t)),
            Action::Commencer(t) => {
                self.saisie = t;
                self.saisie_au_clavier = true;
            }
        }
    }
}

// ---- Ce qu'on demande à Eve ---------------------------------------------------------------------------------------

/// Ce qu'on a choisi dans le menu d'une chose du planning : jamais une écriture dans l'ERP — une demande à Eve, que
/// Claude fera (l'accord de la personne valant là où les accords valent).
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Action {
    /// Une demande à Eve (en parler, déplacer une carte), envoyée comme si on l'avait écrite.
    Demander(String),
    /// Me le rappeler demain : ce que dira le rappel.
    Rappeler(String),
    /// Une demande à compléter (replanifier une tâche) : la saisie la commence.
    Commencer(String),
}

/// Une phrase qui finit par sa ponctuation : un point s'il n'en a pas.
fn avec_un_point(t: &str) -> String {
    let t = t.trim();
    if t.is_empty() || t.ends_with(['.', '!', '?', '…', ')']) { t.to_string() } else { format!("{t}.") }
}

/// Quand a lieu un événement, comme on le dit : « ven. 30 oct. à 7 h », « ven. 30 oct. », « du lun. 2 au ven. 6 nov. ».
pub(super) fn quand_de_l_evenement(e: &Evenement) -> String {
    let debut = centre::jour_court(&e.debut);
    let heure = e.heure.as_deref().filter(|h| !h.trim().is_empty()).map(centre::heure_parlee);
    match (e.fin.as_deref().filter(|f| *f != e.debut), heure) {
        (Some(fin), _) => format!("du {debut} au {}", centre::jour_court(fin)),
        (None, Some(h)) => format!("{debut} à {h}"),
        (None, None) => debut,
    }
}

/// « En parler à Eve » d'un événement du calendrier : son titre, quand, et sa source (ce qu'Eve en dit à Claude) — sans
/// source, son détail.
pub(super) fn parler_de_l_evenement(e: &Evenement) -> Action {
    let quoi = if e.source.trim().is_empty() { e.detail.trim() } else { e.source.trim() };
    let suite = if quoi.is_empty() { String::new() } else { format!(" : {quoi}") };
    Action::Demander(avec_un_point(&format!(
        "Parlons de ceci, au calendrier du tableau de bord (« {} », {}){suite}",
        e.titre.trim(),
        quand_de_l_evenement(e)
    )))
}

/// « Me le rappeler demain » d'un événement : ce que dira le rappel.
pub(super) fn rappel_de_l_evenement(e: &Evenement) -> Action {
    Action::Rappeler(format!("Calendrier : {}, {}", e.titre.trim(), quand_de_l_evenement(e)))
}

/// Les jours d'une tâche, comme on les dit : « du 28 sept. au 30 oct. », « le 5 nov. » (un jalon).
pub(super) fn jours_de_la_tache(t: &Tache) -> String {
    if t.debut == t.fin {
        format!("le {}", centre::date_courte(&t.debut))
    } else {
        format!("du {} au {}", centre::date_courte(&t.debut), centre::date_courte(&t.fin))
    }
}

/// Ce qui situe une tâche dans une demande : son titre et son chantier (`parent` : son titre), ses jours, son statut.
fn tache_dite(t: &Tache, parent: Option<&str>) -> String {
    let chantier = parent.map_or(String::new(), |p| format!(", chantier « {p} »"));
    let statut =
        if t.statut.trim().is_empty() { String::new() } else { format!(", {}", t.statut.trim().to_lowercase()) };
    format!("« {} »{chantier}, {}{statut}", t.titre.trim(), jours_de_la_tache(t))
}

/// « En parler à Eve » d'une tâche du Gantt.
pub(super) fn parler_de_la_tache(t: &Tache, parent: Option<&str>) -> Action {
    let source = if t.source.trim().is_empty() { String::new() } else { format!(" : {}", t.source.trim()) };
    Action::Demander(avec_un_point(&format!(
        "Parlons de ceci, au Gantt du tableau de bord ({}){source}",
        tache_dite(t, parent)
    )))
}

/// « Me le rappeler demain » d'une tâche.
pub(super) fn rappel_de_la_tache(t: &Tache, parent: Option<&str>) -> Action {
    let chantier = parent.map_or(String::new(), |p| format!(" ({p})"));
    Action::Rappeler(format!("Gantt : {}{chantier}, {}", t.titre.trim(), jours_de_la_tache(t)))
}

/// « Replanifier… » d'une tâche : la saisie commence la demande, la personne dit les nouvelles dates.
pub(super) fn replanifier_la_tache(t: &Tache, parent: Option<&str>) -> Action {
    Action::Commencer(format!("Replanifie dans Constructo AI la tâche {} : ", tache_dite(t, parent)))
}

/// « En parler à Eve » d'une carte du Kanban : son tableau, sa colonne, sa source (sans elle, son titre et son détail).
pub(super) fn parler_de_la_carte(c: &Carte, tableau: &str, colonne: &str) -> Action {
    let quoi = if c.source.trim().is_empty() {
        [c.titre.trim(), c.detail.trim()].into_iter().filter(|x| !x.is_empty()).collect::<Vec<_>>().join(", ")
    } else {
        c.source.trim().to_string()
    };
    Action::Demander(avec_un_point(&format!(
        "Parlons de ceci, au Kanban du tableau de bord (tableau « {tableau} », colonne « {colonne} ») : {quoi}"
    )))
}

/// « Me le rappeler demain » d'une carte.
pub(super) fn rappel_de_la_carte(c: &Carte, tableau: &str, colonne: &str) -> Action {
    Action::Rappeler(format!("Kanban {tableau} : {} ({colonne})", c.titre.trim()))
}

/// « Déplacer vers… » une autre colonne : une demande à Eve, qui le fera dans Constructo AI — avec la règle du tableau
/// (`garde`), qu'elle doit suivre, et la source de la carte.
pub(super) fn deplacer_la_carte(c: &Carte, tableau: &str, de: &str, vers: &str, garde: Option<&str>) -> Action {
    let regle =
        garde.filter(|g| !g.trim().is_empty()).map_or(String::new(), |g| format!(" Sa règle : {}", avec_un_point(g)));
    let source = if c.source.trim().is_empty() { String::new() } else { format!(" ({})", c.source.trim()) };
    let titre = c.titre.trim();
    Action::Demander(format!(
        "Dans Constructo AI, déplace la carte « {titre} »{source} du tableau « {tableau} », de « {de} » vers \
         « {vers} ».{regle}"
    ))
}

// ---- Les sortes, les dates --------------------------------------------------------------------------------------

/// La teinte, l'icône et le nom (dans une légende) d'une sorte d'événement, les teintes des cartes du tableau de bord
/// qui disent les mêmes choses : les rendez-vous en violet (« Ta journée »), les notes de l'ERP en indigo (son agenda),
/// les chantiers et leurs opérations en bleu, les échéances et l'école en jaune, les factures en vert, les livraisons
/// en orange, les rappels en sarcelle, les anniversaires en rouge. L'icône dit la sorte quand deux partagent une
/// teinte, et le texte la dit toujours : la couleur ne parle jamais seule.
pub(super) fn allure(sorte: Sorte) -> (erp::Teinte, char, &'static str) {
    match sorte {
        Sorte::RendezVous => (erp::VIOLET, '\u{E716}', "Rendez-vous"),
        Sorte::Note => (erp::INDIGO, pictos::NOTE, "Notes de l'ERP"),
        Sorte::Chantier => (erp::BLEU, pictos::PROJETS, "Chantiers"),
        Sorte::Operation => (erp::BLEU, '\u{E90F}', "Opérations"),
        Sorte::Echeance => (erp::JAUNE, pictos::ECHEANCES, "Échéances"),
        Sorte::Facture => (erp::VERT, pictos::FACTURES_RECUES, "Factures"),
        Sorte::Livraison => (erp::ORANGE, pictos::COMMANDES, "Livraisons"),
        Sorte::Rappel => (erp::SARCELLE, pictos::RAPPELS, "Rappels d'Eve"),
        Sorte::Anniversaire => (erp::ROUGE, '\u{ED55}', "Anniversaires"),
        Sorte::Ecole => (erp::JAUNE, '\u{E7BE}', "École"),
        Sorte::Autre => (erp::GRIS, pictos::INFO, "Autres"),
    }
}

/// L'allure d'un événement à un jour donné : celle de sa sorte ; une facture échue (son jour passé), le rouge de ce
/// qui est en retard et l'icône du retard — un état, pas une sorte.
pub(super) fn allure_de(e: &Evenement, aujourdhui: i64) -> (erp::Teinte, char, &'static str) {
    let echue = e.sorte == Sorte::Facture && centre::numero_du_jour(&e.debut).is_some_and(|d| d < aujourdhui);
    if echue { (erp::ROUGE, pictos::RETARD, "Factures échues") } else { allure(e.sorte) }
}

/// L'encre d'une icône d'une teinte, lisible sur le blanc.
pub(super) fn encre_de(t: erp::Teinte) -> Color32 {
    theme::lisible(t.vif[1])
}

/// Un texte de l'ERP tel qu'il se lit à l'écran : la fonction du contrat, une seule table d'accents pour les données
/// et pour les vues.
pub(super) use crate::planning::en_francais;

/// Le côté d'une chose se montre-t-il dans cette vue du tableau de bord ?
pub(super) fn se_montre(cote: Cote, vue: Vue) -> bool {
    match vue {
        Vue::Tout => true,
        Vue::Entreprise => cote == Cote::Entreprise,
        Vue::Famille => cote == Cote::Famille,
    }
}

/// Le jour de la semaine d'un jour (son numéro depuis 1970) : 0 lundi … 6 dimanche (le 1er janvier 1970 était un
/// jeudi).
pub(super) fn jour_de_semaine(n: i64) -> i64 {
    (n + 3).rem_euclid(7)
}

/// Le lundi de la semaine d'un jour.
pub(super) fn lundi(n: i64) -> i64 {
    n - jour_de_semaine(n)
}

/// L'année et le mois d'un jour.
pub(super) fn annee_et_mois(n: i64) -> (i64, i64) {
    let d = centre::date_du_numero(n);
    (d[..4].parse().unwrap_or(1970), d[5..7].parse().unwrap_or(1))
}

/// Le premier jour d'un mois (`mois` : 1 à 12 ; au-delà, les années suivent).
pub(super) fn premier_jour(annee: i64, mois: i64) -> i64 {
    let (a, m) = (annee + (mois - 1).div_euclid(12), (mois - 1).rem_euclid(12) + 1);
    centre::numero_du_jour(&format!("{a:04}-{m:02}-01")).unwrap_or(0)
}

/// Le premier jour du mois d'un jour, et celui du mois qui suit.
pub(super) fn bornes_du_mois(n: i64) -> (i64, i64) {
    let (a, m) = annee_et_mois(n);
    (premier_jour(a, m), premier_jour(a, m + 1))
}

/// Le nom du mois d'un jour, en minuscules : « octobre » (d'après la date en toutes lettres, celle de l'ambiance).
pub(super) fn nom_du_mois(n: i64) -> String {
    crate::ambiance::date_longue(&centre::date_du_numero(n)).rsplit(' ').next().unwrap_or("").to_lowercase()
}

/// Le jour du mois d'un jour : 1 à 31.
pub(super) fn quantieme(n: i64) -> i64 {
    centre::date_du_numero(n)[8..10].parse().unwrap_or(1)
}

/// Les trois lettres d'un jour de la semaine, en capitales : « LUN », « MAR »…
pub(super) fn jour_en_trois_lettres(n: i64) -> String {
    crate::ambiance::date_longue(&centre::date_du_numero(n)).chars().take(3).collect()
}

/// Les initiales d'une personne : « Alexis Tremblay » → « AT », « Sam » → « S ».
pub(super) fn initiales(nom: &str) -> String {
    nom.split([' ', '-'])
        .filter_map(|m| m.chars().next())
        .filter(|c| c.is_alphabetic())
        .take(2)
        .flat_map(char::to_uppercase)
        .collect()
}

/// La couleur d'une personne (une pastille d'initiales) : toujours la même pour un même nom, parmi les teintes vives
/// de l'ERP — la couleur suit la personne, jamais son rang.
pub(super) fn couleur_de_la_personne(nom: &str) -> Color32 {
    const TEINTES: [erp::Teinte; 6] = [erp::BLEU, erp::VIOLET, erp::SARCELLE, erp::ORANGE, erp::INDIGO, erp::VERT];
    let n = eve::parole::normaliser(nom).bytes().fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(u32::from(b)));
    TEINTES[(n % TEINTES.len() as u32) as usize].vif[1]
}

// ---- Les pièces communes des vues -------------------------------------------------------------------------------

/// Le titre d'une vue, dans sa barre d'outils (« Octobre 2026 »).
fn titre_de_vue(ui: &mut egui::Ui, texte: &str) {
    let g =
        ui.painter().layout_no_wrap(texte.to_string(), FontId::new(21.0, FontFamily::Name(GRAS.into())), erp::TITRE);
    let (r, _) = ui.allocate_exact_size(vec2(g.size().x, 34.0), Sense::hover());
    ui.painter().galley(pos2(r.left(), r.center().y - g.size().y / 2.0), g, erp::TITRE);
}

/// Un bouton rond de la barre d'outils (‹ et › : avant, après), comme celui qui relit Constructo AI.
fn bouton_rond(ui: &mut egui::Ui, icone: char, aide: &str, cle: &str) -> bool {
    let (r, _) = ui.allocate_exact_size(vec2(34.0, 34.0), Sense::hover());
    let rep = ui.interact(r, egui::Id::unique(("bouton-du-planning", cle)), Sense::click());
    let fond = if rep.hovered() { erp::SURVOL } else { Color32::from_white_alpha(225) };
    ui.painter().add(theme::ombre_marine(r, 17, 1, 4, 14));
    ui.painter().circle(r.center(), 17.0, fond, Stroke::new(1.0, erp::FILET_DE_CARTE));
    let encre = if rep.hovered() { erp::TITRE } else { erp::TEXTE_2 };
    let police = FontId::new(13.0, FontFamily::Name(ICONES.into()));
    ui.painter().text(r.center(), Align2::CENTER_CENTER, icone, police, encre);
    rep.on_hover_text(aide).on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

/// La navigation d'une vue : ‹ (avant), « Aujourd'hui » (désactivé quand la vue y est déjà), › (après). Rend -1, 0
/// (aujourd'hui) ou 1.
fn navigation(ui: &mut egui::Ui, cle: &str, aides: (&str, &str), deja_aujourdhui: bool) -> Option<i64> {
    let mut choix = None;
    if bouton_rond(ui, '\u{E76B}', aides.0, &format!("{cle}-avant")) {
        choix = Some(-1);
    }
    ui.add_space(6.0);
    let sorte = if deja_aujourdhui { BoutonErp::Fantome } else { BoutonErp::Secondaire };
    let rep = theme::bouton_erp_nomme(ui, Some(egui::Id::unique(("aujourd-hui", cle))), None, "Aujourd'hui", sorte);
    if rep.clicked() && !deja_aujourdhui {
        choix = Some(0);
    }
    ui.add_space(6.0);
    if bouton_rond(ui, '\u{E76C}', aides.1, &format!("{cle}-apres")) {
        choix = Some(1);
    }
    choix
}

/// Une légende, sur une rangée qui passe à la ligne : chaque entrée, sa clé (une pastille de sa couleur, ou un trait
/// pour ce qui en est un) et son nom, à l'encre secondaire — l'identité ne tient jamais à la couleur seule.
fn legende(ui: &mut egui::Ui, entrees: &[Cle]) {
    if entrees.is_empty() {
        return;
    }
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(16.0, 6.0);
        for c in entrees {
            let g = ui.painter().layout_no_wrap(c.nom.to_string(), FontId::proportional(12.5), erp::TEXTE_2);
            let devant = if c.icone.is_some() { 34.0 } else { 18.0 };
            let (r, _) = ui.allocate_exact_size(vec2(devant + g.size().x, g.size().y.max(16.0)), Sense::hover());
            let p = ui.painter();
            let centre = pos2(r.left() + 6.0, r.center().y);
            match c.forme {
                FormeDeCle::Pastille => {
                    p.rect_filled(Rect::from_center_size(centre, vec2(11.0, 11.0)), 3.0, c.couleur);
                }
                FormeDeCle::Trait => {
                    p.line_segment([centre - vec2(6.0, 0.0), centre + vec2(6.0, 0.0)], Stroke::new(2.0, c.couleur));
                }
                FormeDeCle::Barre => {
                    p.rect_filled(Rect::from_center_size(centre, vec2(13.0, 4.0)), 2.0, c.couleur);
                }
                FormeDeCle::Verticale => {
                    p.line_segment([centre - vec2(0.0, 7.0), centre + vec2(0.0, 7.0)], Stroke::new(2.0, c.couleur));
                }
            }
            if let Some(i) = c.icone {
                let police = FontId::new(12.0, FontFamily::Name(ICONES.into()));
                p.text(pos2(r.left() + 25.0, r.center().y), Align2::CENTER_CENTER, i, police, erp::TEXTE_2);
            }
            p.galley(pos2(r.left() + devant, r.center().y - g.size().y / 2.0), g, erp::TEXTE_2);
        }
    });
}

/// Une entrée d'une légende : sa couleur, sa forme, son icône (au besoin), son nom.
#[derive(Clone, Debug, PartialEq)]
struct Cle {
    couleur: Color32,
    forme: FormeDeCle,
    icone: Option<char>,
    nom: String,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum FormeDeCle {
    Pastille,
    Trait,
    Barre,
    Verticale,
}

/// Un état vide d'une vue, dans une carte : la pastille de son icône, son titre, ce qu'il en est, et un bouton au
/// besoin. Rend vrai si le bouton est cliqué.
fn etat_vide(
    ui: &mut egui::Ui,
    icone: char,
    teinte: erp::Teinte,
    titre: &str,
    texte: &str,
    bouton: Option<&str>,
) -> bool {
    let mut clic = false;
    theme::carte(ui, Some(teinte.pastel), 0.0, |ui| {
        theme::tete_de_carte(ui, titre, icone, teinte, false, None);
        ui.add(egui::Label::new(egui::RichText::new(texte).size(14.0).color(erp::TEXTE)).wrap().selectable(false));
        if let Some(mot) = bouton {
            ui.add_space(6.0);
            clic = theme::bouton_erp(ui, None, mot, BoutonErp::Secondaire).clicked();
        }
    });
    clic
}

/// La bulle d'un survol, au-dessus de tout, près du pointeur sans sortir de la fenêtre, comme celle des graphiques :
/// son titre en demi-gras, puis ses lignes à l'encre secondaire — chacune sa pastille de couleur au besoin.
fn bulle(ui: &egui::Ui, pres: Pos2, titre: &str, lignes: &[(Option<Color32>, String)], cle: &str) {
    let couche = egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique(("bulle-du-planning", cle)));
    let p = ui.ctx().layer_painter(couche);
    let large = 300.0;
    let g_titre = p.layout(titre.to_string(), FontId::new(13.5, FontFamily::Name(SEMI.into())), erp::TITRE, large);
    let rangs: Vec<(Option<Color32>, Arc<egui::Galley>)> = lignes
        .iter()
        .filter(|(_, t)| !t.trim().is_empty())
        .map(|(c, t)| {
            (
                *c,
                p.layout(
                    t.clone(),
                    FontId::proportional(12.5),
                    erp::TEXTE_2,
                    large - if c.is_some() { 16.0 } else { 0.0 },
                ),
            )
        })
        .collect();
    let largeur =
        rangs.iter().map(|(c, g)| g.size().x + if c.is_some() { 16.0 } else { 0.0 }).fold(g_titre.size().x, f32::max)
            + 24.0;
    let hauteur = 10.0 + g_titre.size().y + rangs.iter().map(|(_, g)| g.size().y + 3.0).sum::<f32>() + 9.0;
    let ecran = ui.ctx().content_rect();
    let mut coin = pres + vec2(16.0, 16.0);
    if coin.x + largeur > ecran.right() - 4.0 {
        coin.x = pres.x - 16.0 - largeur;
    }
    if coin.y + hauteur > ecran.bottom() - 4.0 {
        coin.y = pres.y - 16.0 - hauteur;
    }
    let r = Rect::from_min_size(coin, vec2(largeur, hauteur));
    p.add(theme::ombre_marine(r, 8, 4, 14, 46));
    p.rect(r, 8.0, Color32::WHITE, Stroke::new(1.0, erp::FILET_DE_CARTE), StrokeKind::Inside);
    let mut y = r.top() + 10.0;
    let h_titre = g_titre.size().y;
    p.galley(pos2(r.left() + 12.0, y), g_titre, erp::TITRE);
    y += h_titre + 3.0;
    for (couleur, g) in rangs {
        let h = g.size().y;
        let mut x = r.left() + 12.0;
        if let Some(c) = couleur {
            p.circle_filled(pos2(x + 4.0, y + 8.0), 4.0, c);
            x += 16.0;
        }
        p.galley(pos2(x, y), g, erp::TEXTE_2);
        y += h + 3.0;
    }
}

/// Le menu d'une chose du planning, qu'ouvre un clic sur elle (`rep`) : « En parler à Eve », « Me le rappeler
/// demain », puis ce qu'elle offre en propre (`propres` : une tâche, « Replanifier… »), et une section au besoin (son
/// titre, ses lignes : une carte, les colonnes où la déplacer). `cle` nomme ses lignes. Rend le rang de la ligne
/// choisie : 0 parler, 1 rappeler, puis les propres, puis la section. Il s'ouvre là où l'on a touché la chose : une
/// tâche du Gantt fait toute la largeur de sa carte.
fn menu_de_chose(
    rep: &egui::Response,
    cle: &str,
    propres: &[(char, String)],
    section: Option<(&str, &[(char, String)])>,
) -> Option<usize> {
    let touchee = egui::Id::unique(("chose-touchee", cle));
    if rep.clicked()
        && let Some(p) = rep.interact_pointer_pos()
    {
        rep.ctx.data_mut(|d| d.insert_temp(touchee, p));
    }
    let mut menu = theme::menu_blanc(rep);
    if let Some(p) = rep.ctx.data(|d| d.get_temp::<Pos2>(touchee)) {
        menu = menu.at_position(p);
    }
    let mut choisie = None;
    menu.show(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        let mut lignes: Vec<(char, String)> = vec![
            (pictos::PARLER, "En parler à Eve".to_string()),
            (pictos::RAPPELS, "Me le rappeler demain".to_string()),
        ];
        lignes.extend(propres.iter().cloned());
        let communes = lignes.len();
        for (k, (icone, mot)) in lignes.iter().enumerate() {
            if theme::ligne_du_menu(ui, *icone, mot, false, egui::Id::unique(("ligne-du-planning", cle, k))).clicked() {
                choisie = Some(k);
            }
        }
        if let Some((titre, autres)) = section.filter(|(_, l)| !l.is_empty()) {
            ui.add_space(4.0);
            let (f, _) = ui.allocate_exact_size(vec2(236.0, 1.0), Sense::hover());
            ui.painter().hline(f.x_range(), f.center().y, Stroke::new(1.0, erp::FILET_DE_CARTE));
            let police = FontId::new(11.5, FontFamily::Name(SEMI.into()));
            let (t, _) = ui.allocate_exact_size(vec2(236.0, 24.0), Sense::hover());
            ui.painter().text(pos2(t.left() + 10.0, t.center().y), Align2::LEFT_CENTER, titre, police, erp::TEXTE_3);
            for (j, (icone, mot)) in autres.iter().enumerate() {
                let k = communes + j;
                if theme::ligne_du_menu(ui, *icone, mot, false, egui::Id::unique(("ligne-du-planning", cle, k)))
                    .clicked()
                {
                    choisie = Some(k);
                }
            }
        }
    });
    choisie
}

/// Ce qu'une scène de l'essai du thème montre du planning : l'onglet ; le calendrier à la semaine (sinon au mois) ; le
/// Gantt aux mois (sinon aux semaines), ses chantiers dépliés (par leur id), en tableau (`donnees`) ; le tableau du
/// Kanban (sa clé ; vide : le premier).
pub(crate) struct Montre<'a> {
    pub(crate) onglet: Onglet,
    pub(crate) semaine: bool,
    pub(crate) mois: bool,
    pub(crate) deplies: &'a [&'a str],
    pub(crate) donnees: bool,
    pub(crate) tableau: &'a str,
}

/// Les vues du planning d'une scène de l'essai du thème, autour d'aujourd'hui. La vitrine, jamais Eve.
pub(crate) fn preparer_pour_l_essai(ctx: &egui::Context, m: &Montre<'_>) {
    choisir_l_onglet(ctx, m.onglet);
    calendrier::preparer(ctx, if m.semaine { calendrier::Mode::Semaine } else { calendrier::Mode::Mois });
    gantt::preparer(ctx, m.deplies, m.mois, m.donnees);
    kanban::preparer(ctx, m.tableau);
}

#[cfg(test)]
mod essais {
    use super::*;
    use crate::planning::{Colonne, Tableau};

    fn evenement(debut: &str, fin: Option<&str>, heure: Option<&str>, sorte: Sorte, source: &str) -> Evenement {
        Evenement {
            id: format!("{debut}·{sorte:?}"),
            debut: debut.into(),
            fin: fin.map(Into::into),
            heure: heure.map(Into::into),
            titre: "Livraison des fermes de toit".into(),
            detail: "Entrée nord".into(),
            sorte,
            cote: Cote::Entreprise,
            source: source.into(),
        }
    }

    #[test]
    fn les_jours_et_les_mois() {
        let n = |d: &str| centre::numero_du_jour(d).unwrap();
        assert_eq!(jour_de_semaine(n("2026-10-26")), 0, "un lundi");
        assert_eq!(jour_de_semaine(n("2026-11-01")), 6, "un dimanche");
        assert_eq!(lundi(n("2026-10-30")), n("2026-10-26"));
        assert_eq!(bornes_du_mois(n("2026-10-30")), (n("2026-10-01"), n("2026-11-01")));
        assert_eq!(bornes_du_mois(n("2026-12-15")), (n("2026-12-01"), n("2027-01-01")), "décembre, puis janvier");
        assert_eq!(premier_jour(2026, 0), n("2025-12-01"), "le mois d'avant janvier");
        assert_eq!(nom_du_mois(n("2026-10-30")), "octobre");
        assert_eq!(quantieme(n("2026-10-30")), 30);
        assert_eq!(jour_en_trois_lettres(n("2026-10-28")), "MER");
        assert_eq!(initiales("Alexis Tremblay"), "AT");
        assert_eq!(initiales("Marc-André Gagné"), "MA");
        assert_eq!(initiales("Sam"), "S");
        assert_eq!(couleur_de_la_personne("Alexis Tremblay"), couleur_de_la_personne("alexis tremblay"));
    }

    #[test]
    fn les_demandes_d_un_evenement_portent_sa_source() {
        let source = "la livraison LIV-2026-0439, fermes de toit, entrepôt Exemple";
        let e = evenement("2026-11-02", None, Some("08:00"), Sorte::Livraison, source);
        assert_eq!(quand_de_l_evenement(&e), "lun. 2 nov. à 8 h");
        let Action::Demander(d) = parler_de_l_evenement(&e) else { panic!("une demande") };
        assert!(d.contains("« Livraison des fermes de toit », lun. 2 nov. à 8 h") && d.contains(source), "{d}");
        assert!(d.ends_with('.'), "{d}");
        assert_eq!(
            rappel_de_l_evenement(&e),
            Action::Rappeler("Calendrier : Livraison des fermes de toit, lun. 2 nov. à 8 h".into())
        );
        let sur_trois_jours = evenement("2026-11-02", Some("2026-11-04"), None, Sorte::Operation, "");
        assert_eq!(quand_de_l_evenement(&sur_trois_jours), "du lun. 2 nov. au mer. 4 nov.");
        let Action::Demander(d) = parler_de_l_evenement(&sur_trois_jours) else { panic!() };
        assert!(d.ends_with(" : Entrée nord."), "sans source, son détail : {d}");
    }

    #[test]
    fn une_facture_passee_se_dit_echue() {
        let aujourdhui = centre::numero_du_jour("2026-10-30").unwrap();
        let passee = evenement("2026-10-25", None, None, Sorte::Facture, "");
        let a_venir = evenement("2026-11-02", None, None, Sorte::Facture, "");
        assert_eq!(allure_de(&passee, aujourdhui).2, "Factures échues");
        assert_eq!(allure_de(&a_venir, aujourdhui), allure(Sorte::Facture));
    }

    #[test]
    fn les_demandes_d_une_tache_et_d_une_carte() {
        let t = Tache {
            id: "op-1".into(),
            parent: Some("ch-1".into()),
            titre: "Structure d'acier".into(),
            debut: "2026-09-28".into(),
            fin: "2026-10-30".into(),
            avancement: Some(0.85),
            statut: "En cours".into(),
            prevu: None,
            responsables: vec!["Alexis Tremblay".into()],
            source: "l'opération 12 du projet PROJ-2026-00135".into(),
        };
        let Action::Demander(d) = parler_de_la_tache(&t, Some("Entrepôt Exemple")) else { panic!() };
        assert!(
            d.contains("« Structure d'acier », chantier « Entrepôt Exemple », du 28 sept. au 30 oct., en cours"),
            "{d}"
        );
        assert!(d.contains("PROJ-2026-00135"), "{d}");
        let Action::Commencer(r) = replanifier_la_tache(&t, Some("Entrepôt Exemple")) else { panic!() };
        assert!(
            r.starts_with("Replanifie dans Constructo AI la tâche « Structure d'acier »") && r.ends_with(" : "),
            "{r}"
        );
        let jalon = Tache { debut: "2026-11-05".into(), fin: "2026-11-05".into(), ..t.clone() };
        assert_eq!(jours_de_la_tache(&jalon), "le 5 nov.");
        let c = Carte {
            id: "DEV-2026-0214".into(),
            titre: "Centre de la petite enfance".into(),
            detail: "CPE Exemple".into(),
            montant: Some(1_450_000.0),
            echeance: None,
            responsables: Vec::new(),
            source: "la soumission DEV-2026-0214".into(),
        };
        let tableau = Tableau {
            cle: "soumissions".into(),
            titre: "Soumissions".into(),
            colonnes: vec![Colonne {
                statut: "ENVOYE".into(),
                titre: "Envoyée".into(),
                total: 1,
                cartes: vec![c.clone()],
            }],
            garde: Some("Le statut suit celui de la soumission".into()),
        };
        let Action::Demander(d) =
            deplacer_la_carte(&c, &tableau.titre, "Envoyée", "Acceptée", tableau.garde.as_deref())
        else {
            panic!()
        };
        assert!(d.contains("de « Envoyée » vers « Acceptée »") && d.contains("(la soumission DEV-2026-0214)"), "{d}");
        assert!(d.ends_with("Sa règle : Le statut suit celui de la soumission."), "{d}");
        let Action::Demander(d) = parler_de_la_carte(&c, "Soumissions", "Envoyée") else { panic!() };
        assert!(d.ends_with("colonne « Envoyée ») : la soumission DEV-2026-0214."), "{d}");
        assert_eq!(
            rappel_de_la_carte(&c, "Soumissions", "Envoyée"),
            Action::Rappeler("Kanban Soumissions : Centre de la petite enfance (Envoyée)".into())
        );
    }

    /// Le menu d'une chose, sans fenêtre : un clic sur ce qui l'ouvre, puis sur « En parler à Eve », rend sa ligne ;
    /// une section (« Déplacer vers… ») a ses lignes après les communes.
    #[test]
    fn le_menu_d_une_chose_rend_la_ligne_choisie() {
        let ctx = egui::Context::default();
        crate::natif::polices(&ctx);
        let colonnes = [('\u{E72A}', "En cours".to_string()), ('\u{E72A}', "Terminé".to_string())];
        let mut t = 0.0;
        let mut image = |evenements: Vec<egui::Event>| {
            t += 0.1;
            let entree = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(600.0, 500.0))),
                time: Some(t),
                events: evenements,
                ..Default::default()
            };
            let mut choisie = None;
            let _ = ctx.run_ui(entree, |ui| {
                let (r, _) = ui.allocate_exact_size(vec2(160.0, 30.0), Sense::hover());
                let rep = ui.interact(r, egui::Id::unique("une-chose-a-toucher"), Sense::click());
                choisie = menu_de_chose(&rep, "une-chose", &[], Some(("DÉPLACER VERS…", &colonnes)));
            });
            choisie
        };
        let bouton = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        image(Vec::new());
        let c = ctx.read_response(egui::Id::unique("une-chose-a-toucher")).expect("la chose").rect.center();
        image(vec![egui::Event::PointerMoved(c)]);
        image(vec![bouton(c, true)]);
        assert_eq!(image(vec![bouton(c, false)]), None);
        image(Vec::new());
        image(Vec::new());
        let ligne = |k: usize| ctx.read_response(egui::Id::unique(("ligne-du-planning", "une-chose", k)));
        assert!(ligne(3).is_some(), "les colonnes après les deux lignes communes");
        let terminee = ligne(3).unwrap().rect.center();
        image(vec![egui::Event::PointerMoved(terminee)]);
        image(vec![bouton(terminee, true)]);
        assert_eq!(image(vec![bouton(terminee, false)]), Some(3));
    }

    #[test]
    fn chaque_vue_garde_son_cote() {
        assert!(se_montre(Cote::Famille, Vue::Tout) && se_montre(Cote::Entreprise, Vue::Tout));
        assert!(se_montre(Cote::Entreprise, Vue::Entreprise) && !se_montre(Cote::Famille, Vue::Entreprise));
        assert!(se_montre(Cote::Famille, Vue::Famille) && !se_montre(Cote::Entreprise, Vue::Famille));
    }
}
