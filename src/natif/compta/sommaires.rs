//! Les sommaires de l'onglet « Comptabilité » : par mois, par fournisseur, par chantier, calculés par
//! `crate::compta::sommaires`, en tableaux dans une carte du thème — l'en-tête en petites capitales dans son bandeau
//! marine (comme les tableaux des cartes, `cartes.rs`), ses groupes de colonnes au-dessus, les nombres à droite, la
//! rangée TOTAL au pied ; et, comme EG (`.c-barre`, « Part du plus grand total »), une barre dans chaque rangée.
//! L'en-tête et le TOTAL restent en vue quand un long tableau défile (EG : `position: sticky`). Une fenêtre étroite les
//! montre en rangées de deux lignes : le nom et le chiffre principal, puis les autres chiffres.
//!
//! Un clic sur une rangée mène à ses factures : un mois, à la liste des fournisseurs de ce mois (ses revenus, à celle
//! des clients) ; un fournisseur, à ses factures ; un chantier devient celui d'en tête, et la vue revient au tableau de
//! bord. Rien ne s'écrit dans l'ERP.
//!
//! Le tableau se calcule et se met en mots une fois par lecture, par filtre et par jour (`TableauEnMemoire`, dans la
//! mémoire d'egui) ; seules les rangées à l'écran se dessinent.

use std::hash::{Hash, Hasher};
use std::sync::Arc;

use eframe::egui::{self, Color32, CornerRadius, FontFamily, FontId, Rect, Sense, Stroke, pos2, vec2};

use super::super::Fenetre;
use super::super::graphiques::SERIES;
use super::super::planning::etat_vide;
use super::super::theme::{self, erp};
use super::super::{SEMI, insecables};
use super::{Aller, Choix, Contexte, VueCompta};
use crate::compta::sommaires::{self as calcul, ChantierLigne, Mois};
use crate::compta::{Cents, Chantier, Filtres, MOIS, Registre, Somme, argent};

// ---- Les couleurs ---------------------------------------------------------------------------------------------------

/// Les dépenses : le bleu des barres d'EG (`--bar`, #2A78D6), le premier de la palette des graphiques.
const DEPENSES: Color32 = SERIES[0];
/// Les revenus : le vert d'eau d'EG (`--rev`, #1A9A6C : contraste de 3:1 sur le blanc, vérifié par EG au validateur de
/// sa palette), jamais le vert d'un état (« réglée »).
const REVENUS: Color32 = Color32::from_rgb(0x1A, 0x9A, 0x6C);
/// Le bandeau de l'en-tête, comme celui des tableaux des cartes en clair (le marine de l'assistant de l'ERP).
const BANDEAU: Color32 = erp::MARINE;
/// Le filet fort, sous le TOTAL (celui des tableaux des cartes en clair).
const FILET_FORT: Color32 = Color32::from_rgb(0xC2, 0xCE, 0xDD);
/// Le fond de la rangée TOTAL (EG : `--surface-2`).
const FOND_DU_TOTAL: Color32 = Color32::from_rgb(0xF3, 0xF5, 0xF8);

// ---- Le tableau, en mots --------------------------------------------------------------------------------------------

/// Ce que fait un clic sur une rangée.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Cible {
    /// La liste des factures des fournisseurs : d'un mois, ou d'un fournisseur.
    Fournisseurs { mois: Option<String>, tiers: Option<String> },
    /// La liste des factures des clients d'un mois.
    Clients { mois: String },
    /// Le chantier d'en tête devient celui-là ; la vue revient au tableau de bord.
    Chantier(Chantier),
}

/// L'encre d'une case : ordinaire ; un montant négatif (une perte, un crédit) ; discrète (rien : « — »).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Encre {
    Normale,
    Negative,
    Discrete,
}

#[derive(Clone, Debug)]
struct Case {
    texte: String,
    encre: Encre,
}

impl Case {
    fn de(texte: impl Into<String>) -> Case {
        Case { texte: texte.into(), encre: Encre::Normale }
    }

    /// Un montant : négatif, de l'encre d'une perte.
    fn argent(c: Cents) -> Case {
        Case { texte: argent(c), encre: if c < 0 { Encre::Negative } else { Encre::Normale } }
    }

    /// Rien à dire : un tiret discret.
    fn rien() -> Case {
        Case { texte: "\u{2014}".into(), encre: Encre::Discrete }
    }

    /// Un montant d'une somme, ou un tiret quand elle ne compte aucune facture.
    fn somme(s: &Somme, c: Cents) -> Case {
        if s.n == 0 { Case::rien() } else { Case::argent(c) }
    }
}

/// Une colonne : son groupe (au-dessus, sur les colonnes voisines du même groupe ; vide : aucun), son titre, son
/// alignement.
#[derive(Clone, Debug)]
pub(super) struct Colonne {
    pub(super) groupe: &'static str,
    pub(super) titre: String,
    pub(super) droite: bool,
}

fn colonne(groupe: &'static str, titre: impl Into<String>) -> Colonne {
    Colonne { groupe, titre: titre.into(), droite: true }
}

/// Une rangée : ses cases (une par colonne), ses barres (leur part de la plus grande, de 0 à 1, et leur couleur), ce
/// qu'un clic fait (et, sur certaines colonnes, autre chose : les revenus d'un mois mènent aux clients), sa bulle ; et
/// en fenêtre étroite, son chiffre principal et le reste en une ligne.
#[derive(Clone, Debug)]
struct Rangee {
    cases: Vec<Case>,
    barres: Vec<(f32, Color32)>,
    cible: Option<Cible>,
    cible_des_colonnes: Option<(std::ops::Range<usize>, Cible, String)>,
    aide: String,
    principal: Case,
    reste: String,
}

/// Un sommaire mis en mots : sa carte (son titre, son icône, sa teinte, son badge, ce qu'il montre), ses colonnes, ses
/// rangées et son TOTAL, la légende de ses barres, ses notes ; et, par colonne, les textes qui peuvent être les plus
/// larges (`candidats` : sa largeur se mesure sur eux).
struct Tableau {
    titre: &'static str,
    icone: char,
    teinte: erp::Teinte,
    badge: String,
    sous_titre: &'static str,
    /// Ce que dit le chiffre de droite des rangées de deux lignes (une fenêtre étroite).
    principal: &'static str,
    colonnes: Vec<Colonne>,
    rangees: Vec<Rangee>,
    total: Rangee,
    legende: Vec<(Color32, &'static str)>,
    notes: Vec<String>,
    candidats: Vec<Vec<String>>,
}

/// « 3 factures », « 1 facture ».
fn pluriel(n: u32, un: &str, plusieurs: &str) -> String {
    format!("{}\u{a0}{}", crate::compta::par_milliers(u64::from(n)), if n > 1 { plusieurs } else { un })
}

/// « 2026-10 » → « Octobre 2026 ».
fn mois_dit(mois: &str) -> String {
    let m: usize = mois.get(5..7).and_then(|m| m.parse().ok()).filter(|m| (1..=12).contains(m)).unwrap_or(0);
    match m {
        0 => mois.to_string(),
        m => {
            let nom = MOIS[m - 1];
            let mut c = nom.chars();
            let nom: String = c.next().map(|p| p.to_uppercase().chain(c).collect()).unwrap_or_default();
            format!("{nom} {}", mois.get(..4).unwrap_or(""))
        }
    }
}

/// Les noms des deux taxes, pour les en-têtes : la règle du contrat (`compta::noms_des_taxes`) ; une seule taxe
/// nommée, la colonne de la seconde se dit « — » (la QA5 du lot 13.2 : une entreprise de l'Ontario lisait « TVQ »).
fn noms_des_taxes(r: &Registre) -> (String, String) {
    let (un, deux) = crate::compta::noms_des_taxes(r);
    (un, if deux.is_empty() { "\u{2014}".into() } else { deux })
}

/// La part d'un montant dans le plus grand (une longueur de barre, de 0 à 1) : en millièmes calculés sur les centimes,
/// le flottant seulement au bout.
fn longueur(c: Cents, plus_grand: Cents) -> f32 {
    if c <= 0 || plus_grand <= 0 {
        return 0.0;
    }
    let milliemes = (i128::from(c) * 1000 / i128::from(plus_grand)).min(1000);
    milliemes as f32 / 1000.0
}

/// Le reste d'une rangée en fenêtre étroite : ses morceaux non vides, séparés par « · ».
fn en_ligne(morceaux: &[String]) -> String {
    morceaux.iter().filter(|m| !m.trim().is_empty()).cloned().collect::<Vec<_>>().join(" · ")
}

/// Les textes d'une colonne qui peuvent être les plus larges : ceux qui ont le plus de caractères, ou presque (les
/// chiffres n'ont pas tous la même largeur : « 88 094,72 $ » est plus large que « 14 792,43 $ »), seize au plus.
pub(super) fn candidats<'a>(textes: impl Iterator<Item = &'a str> + Clone) -> Vec<String> {
    let n = textes.clone().map(|t| t.chars().count()).max().unwrap_or(0);
    let mut l: Vec<String> = Vec::new();
    for t in textes.filter(|t| t.chars().count() + 2 >= n) {
        if l.len() >= 16 {
            break;
        }
        if !l.iter().any(|x| x == t) {
            l.push(t.to_string());
        }
    }
    l
}

impl Tableau {
    /// Les textes de chaque colonne sur lesquels sa largeur se mesure : les plus longs, et toujours celui du TOTAL (en
    /// demi-gras, plus large).
    fn mesurer(&mut self) {
        self.candidats = (0..self.colonnes.len())
            .map(|k| {
                let textes = self.rangees.iter().filter_map(move |r| r.cases.get(k)).map(|c| c.texte.as_str());
                let mut l = candidats(textes);
                if let Some(c) = self.total.cases.get(k)
                    && !l.contains(&c.texte)
                {
                    l.push(c.texte.clone());
                }
                l
            })
            .collect();
    }
}

/// Le sommaire par mois.
fn par_mois(c: &Contexte<'_>) -> Tableau {
    let s = calcul::par_mois(c.registre, &c.filtres, c.aujourdhui);
    let (tps, tvq) = noms_des_taxes(c.registre);
    let plus_grand = s.rangees.iter().map(|m| m.depenses.total.max(m.revenus.total)).max().unwrap_or(0);
    let rangee = |m: &Mois, total: bool| {
        let nom = match (&m.mois, total) {
            (_, true) => "TOTAL".to_string(),
            (Some(x), false) => mois_dit(x),
            (None, false) => "Sans date".to_string(),
        };
        let marge = if m.depenses.n + m.revenus.n == 0 { Case::rien() } else { Case::argent(m.marge()) };
        let cases = vec![
            Case::de(nom.clone()),
            Case::de(m.depenses.n.to_string()),
            Case::somme(&m.depenses, m.depenses.sous_total),
            Case::somme(&m.depenses, m.depenses.tps),
            Case::somme(&m.depenses, m.depenses.tvq),
            Case::somme(&m.depenses, m.depenses.total),
            Case::somme(&m.revenus, m.revenus.sous_total),
            Case::somme(&m.revenus, m.revenus.total),
            marge.clone(),
        ];
        let (cible, cible_des_colonnes, aide) = match (&m.mois, total) {
            (Some(x), false) => {
                let clients = Cible::Clients { mois: x.clone() };
                let aide_clients = format!("Voir les factures des clients : {}", nom.to_lowercase());
                if m.depenses.n == 0 {
                    // Un mois qui n'a que des revenus mène aux factures des clients : il n'y a là aucune facture de
                    // fournisseur (comme EG).
                    (Some(clients), None, aide_clients)
                } else {
                    let fournisseurs = Cible::Fournisseurs { mois: Some(x.clone()), tiers: None };
                    let aide = format!("Voir les factures des fournisseurs : {}", nom.to_lowercase());
                    let revenus = (m.revenus.n > 0).then(|| (6..8, clients, aide_clients));
                    (Some(fournisseurs), revenus, aide)
                }
            }
            (None, false) => (None, None, "Les factures sans date ne se filtrent pas par mois.".to_string()),
            _ => (None, None, String::new()),
        };
        let reste = en_ligne(&[
            if m.revenus.n > 0 { format!("revenus {}", argent(m.revenus.total)) } else { "aucun revenu".into() },
            if m.depenses.n + m.revenus.n > 0 { format!("marge {}", argent(m.marge())) } else { String::new() },
            pluriel(m.depenses.n, "facture reçue", "factures reçues"),
            if m.depenses.n > 0 { format!("{tps} {}", argent(m.depenses.tps)) } else { String::new() },
            if m.depenses.n > 0 { format!("{tvq} {}", argent(m.depenses.tvq)) } else { String::new() },
        ]);
        Rangee {
            barres: vec![
                (longueur(m.depenses.total, plus_grand), DEPENSES),
                (longueur(m.revenus.total, plus_grand), REVENUS),
            ],
            principal: Case::somme(&m.depenses, m.depenses.total),
            cases,
            cible,
            cible_des_colonnes,
            aide,
            reste,
        }
    };
    let rangees: Vec<Rangee> = s.rangees.iter().map(|m| rangee(m, false)).collect();
    let mut total = rangee(&s.total, true);
    total.barres.clear();
    let mut notes = vec![
        "Sans les factures annulées ni les pro forma. Les brouillons des fournisseurs comptent (la facture est \
         reçue) ; ceux des clients non (elle n'est pas encore émise). Une note de crédit se retranche. Touche un mois \
         pour voir les factures de ses fournisseurs ; ses revenus, pour celles de ses clients."
            .to_string(),
    ];
    let sans = s.total.depenses.sans_total + s.total.revenus.sans_total;
    if sans > 0 {
        notes.push(format!(
            "{} sans total dans Constructo AI : {} pour zéro dollar, à compléter.",
            pluriel(sans, "facture", "factures"),
            if sans > 1 { "elles comptent" } else { "elle compte" }
        ));
    }
    let n = s.rangees.iter().filter(|m| m.mois.is_some()).count() as u32;
    let mut t = Tableau {
        titre: "Par mois",
        icone: '\u{E787}',
        teinte: erp::BLEU,
        badge: pluriel(n, "mois", "mois"),
        principal: "Dépenses",
        sous_titre: "Les dépenses (les factures des fournisseurs) et les revenus (celles des clients), le mois le plus \
                     récent en haut ; la marge avant taxes.",
        colonnes: vec![
            Colonne { groupe: "", titre: "Mois".into(), droite: false },
            colonne("Dépenses", "Factures"),
            colonne("Dépenses", "Avant taxes"),
            colonne("Dépenses", tps),
            colonne("Dépenses", tvq),
            colonne("Dépenses", "Total"),
            colonne("Revenus", "Avant taxes"),
            colonne("Revenus", "Total"),
            colonne("Marge", "Avant taxes"),
        ],
        rangees,
        total,
        legende: vec![(DEPENSES, "Dépenses"), (REVENUS, "Revenus")],
        notes,
        candidats: Vec::new(),
    };
    t.mesurer();
    t
}

/// Le sommaire par fournisseur.
fn par_fournisseur(c: &Contexte<'_>) -> Tableau {
    let s = calcul::par_fournisseur(c.registre, &c.filtres, c.aujourdhui);
    let (tps, tvq) = noms_des_taxes(c.registre);
    let plus_grand = s.rangees.iter().map(|l| l.depenses.total).max().unwrap_or(0);
    let rangee = |l: &calcul::Fournisseur, total: bool| {
        let nom = match (total, l.nom.as_str()) {
            (true, _) => "TOTAL".to_string(),
            (false, "") => "(Sans fournisseur)".to_string(),
            (false, n) => n.to_string(),
        };
        let part = if total { Some(1000) } else { s.part(l) };
        let part_dite = part.map(calcul::pour_cent);
        let cases = vec![
            Case::de(nom.clone()),
            Case::de(l.depenses.n.to_string()),
            Case::argent(l.depenses.sous_total),
            Case::argent(l.depenses.tps),
            Case::argent(l.depenses.tvq),
            Case::argent(l.depenses.total),
            if l.reste > 0 { Case::argent(l.reste) } else { Case::rien() },
            part_dite.clone().map_or_else(Case::rien, Case::de),
        ];
        let (cible, aide) = match (total, l.nom.is_empty()) {
            (false, false) => (
                Some(Cible::Fournisseurs { mois: None, tiers: Some(l.nom.clone()) }),
                format!("Voir les factures de {}", l.nom),
            ),
            (false, true) => (None, "Ces factures n'ont pas de fournisseur dans Constructo AI.".to_string()),
            (true, _) => (None, String::new()),
        };
        let reste = en_ligne(&[
            pluriel(l.depenses.n, "facture", "factures"),
            if l.reste > 0 { format!("reste à payer {}", argent(l.reste)) } else { "rien à payer".into() },
            part_dite.map_or_else(String::new, |p| format!("{p} des dépenses")),
        ]);
        Rangee {
            barres: if total { Vec::new() } else { vec![(longueur(l.depenses.total, plus_grand), DEPENSES)] },
            principal: Case::argent(l.depenses.total),
            cases,
            cible,
            cible_des_colonnes: None,
            aide,
            reste,
        }
    };
    let rangees: Vec<Rangee> = s.rangees.iter().map(|l| rangee(l, false)).collect();
    let total = rangee(&s.total, true);
    let mut notes = vec![
        "Les factures des fournisseurs, brouillons compris, sans les annulées ni les pro forma ; une note de crédit se \
         retranche. Le reste à payer : le solde des factures, sans les brouillons. La part : celle du total des \
         dépenses, taxes comprises. Touche un fournisseur pour voir ses factures."
            .to_string(),
    ];
    if s.total.depenses.sans_total > 0 {
        let n = s.total.depenses.sans_total;
        notes.push(format!(
            "{} sans total dans Constructo AI : {} pour zéro dollar, à compléter.",
            pluriel(n, "facture", "factures"),
            if n > 1 { "elles comptent" } else { "elle compte" }
        ));
    }
    let mut t = Tableau {
        titre: "Par fournisseur",
        icone: theme::pictos::FACTURES_RECUES,
        teinte: erp::INDIGO,
        badge: pluriel(s.rangees.len() as u32, "fournisseur", "fournisseurs"),
        principal: "Total",
        sous_titre: "Les dépenses de chaque fournisseur, du plus gros au plus petit, et ce qui reste à lui payer.",
        colonnes: vec![
            Colonne { groupe: "", titre: "Fournisseur".into(), droite: false },
            colonne("", "Factures"),
            colonne("", "Avant taxes"),
            colonne("", tps),
            colonne("", tvq),
            colonne("", "Total"),
            colonne("", "Reste à payer"),
            colonne("", "Part"),
        ],
        rangees,
        total,
        legende: Vec::new(),
        notes,
        candidats: Vec::new(),
    };
    t.mesurer();
    t
}

/// Le sommaire par chantier.
fn par_chantier(c: &Contexte<'_>) -> Tableau {
    let s = calcul::par_chantier(c.registre, &c.filtres, c.aujourdhui);
    let plus_grand = s.rangees.iter().map(|l| l.depenses.sous_total.max(l.revenus.sous_total)).max().unwrap_or(0);
    let rangee = |l: &ChantierLigne, total: bool| {
        let nom = if total { "TOTAL".to_string() } else { l.nom.clone() };
        let taux = l.taux_de_marge().map(calcul::pour_cent);
        let mut marge = Case::argent(l.marge());
        if l.n() == 0 {
            marge = Case::rien();
        }
        let cases = vec![
            Case::de(nom.clone()),
            Case::somme(&l.depenses, l.depenses.sous_total),
            Case::somme(&l.revenus, l.revenus.sous_total),
            marge,
            taux.clone().map_or_else(Case::rien, |t| Case {
                encre: if l.marge() < 0 { Encre::Negative } else { Encre::Normale },
                texte: t,
            }),
            if l.a_recevoir > 0 { Case::argent(l.a_recevoir) } else { Case::rien() },
            Case::de(l.n().to_string()),
        ];
        let (cible, aide) = if total {
            (None, String::new())
        } else {
            let aide = match l.chantier {
                Chantier::Aucun => "Voir le tableau de bord des factures sans chantier".to_string(),
                _ => format!("Voir le tableau de bord de ce chantier : {}", l.nom),
            };
            (Some(Cible::Chantier(l.chantier)), aide)
        };
        let reste = en_ligne(&[
            format!("dépenses {}", argent(l.depenses.sous_total)),
            format!("revenus {}", argent(l.revenus.sous_total)),
            taux.map_or_else(String::new, |t| format!("marge {t}")),
            if l.a_recevoir > 0 { format!("à recevoir {}", argent(l.a_recevoir)) } else { String::new() },
            pluriel(l.n(), "facture", "factures"),
        ]);
        Rangee {
            barres: if total {
                Vec::new()
            } else {
                vec![
                    (longueur(l.depenses.sous_total, plus_grand), DEPENSES),
                    (longueur(l.revenus.sous_total, plus_grand), REVENUS),
                ]
            },
            principal: if l.n() == 0 { Case::rien() } else { Case::argent(l.marge()) },
            cases,
            cible,
            cible_des_colonnes: None,
            aide,
            reste,
        }
    };
    let rangees: Vec<Rangee> = s.rangees.iter().map(|l| rangee(l, false)).collect();
    let total = rangee(&s.total, true);
    let mut notes = vec![
        "La marge se calcule avant taxes : les revenus avant taxes, moins les dépenses avant taxes (la TPS et la \
         TVQ se remettent ou se récupèrent ; elles n'en font pas partie). Son taux : la marge sur les revenus. À \
         recevoir : le solde des factures émises aux clients. Touche un chantier pour voir son tableau de bord."
            .to_string(),
    ];
    if s.total.sans_sous_total > 0 {
        let n = s.total.sans_sous_total;
        notes.push(format!(
            "{} sans montant avant taxes dans Constructo AI : la marge ne {} voit pas, à compléter.",
            pluriel(n, "facture", "factures"),
            if n > 1 { "les" } else { "la" }
        ));
    }
    let n = s.rangees.iter().filter(|l| matches!(l.chantier, Chantier::Projet(_))).count() as u32;
    let mut t = Tableau {
        titre: "Par chantier",
        icone: '\u{E80F}',
        teinte: erp::SARCELLE,
        badge: pluriel(n, "chantier", "chantiers"),
        principal: "Marge avant taxes",
        sous_titre: "Les dépenses, les revenus et la marge de chaque chantier, avant taxes ; ce qui reste à recevoir.",
        colonnes: vec![
            Colonne { groupe: "", titre: "Chantier".into(), droite: false },
            colonne("Avant taxes", "Dépenses"),
            colonne("Avant taxes", "Revenus"),
            colonne("Avant taxes", "Marge"),
            colonne("Avant taxes", "Taux"),
            colonne("", "À recevoir"),
            colonne("", "Factures"),
        ],
        rangees,
        total,
        legende: vec![(DEPENSES, "Dépenses"), (REVENUS, "Revenus")],
        notes,
        candidats: Vec::new(),
    };
    t.mesurer();
    t
}

// ---- La mémoire -----------------------------------------------------------------------------------------------------

/// Le tableau d'une vue, gardé dans la mémoire d'egui avec ce qui l'a fait (l'empreinte de la lecture, des filtres et
/// du jour) : il ne se refait que si elle change.
#[derive(Clone)]
struct TableauEnMemoire(u64, Arc<Tableau>);

/// L'empreinte de ce dont dépend un sommaire : la vue, les filtres, la lecture (son heure et son nombre de factures)
/// et le jour (les douze derniers mois en dépendent).
fn empreinte(vue: VueCompta, filtres: Filtres, c: &Contexte<'_>) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (vue, filtres, c.lecture.lu_le.as_str(), c.registre.factures.len(), c.aujourdhui).hash(&mut h);
    h.finish()
}

fn tableau(ctx: &egui::Context, c: &Contexte<'_>, vue: VueCompta) -> Arc<Tableau> {
    let id = egui::Id::unique(("compta-sommaire", vue));
    let cle = empreinte(vue, c.filtres, c);
    if let Some(TableauEnMemoire(k, t)) = ctx.data(|d| d.get_temp::<TableauEnMemoire>(id))
        && k == cle
    {
        return t;
    }
    let t = Arc::new(match vue {
        VueCompta::ParMois => par_mois(c),
        VueCompta::ParFournisseur => par_fournisseur(c),
        _ => par_chantier(c),
    });
    ctx.data_mut(|d| d.insert_temp(id, TableauEnMemoire(cle, t.clone())));
    t
}

// ---- La page --------------------------------------------------------------------------------------------------------

pub(super) fn page(f: &mut Fenetre, ui: &mut egui::Ui, c: &Contexte<'_>, vue: VueCompta) {
    let _ = f;
    let t = tableau(ui.ctx(), c, vue);
    if t.rangees.is_empty() {
        let texte = "Aucune facture de la période et du chantier choisis n'entre dans ce sommaire (une dépense ou un \
                     revenu) : choisis une autre période, ou tous les chantiers.";
        etat_vide(ui, t.icone, t.teinte, t.titre, texte, None);
        return;
    }
    let mut cible = None;
    theme::carte(ui, Some(t.teinte.pastel), 0.0, |ui| {
        theme::tete_de_carte(ui, t.titre, t.icone, t.teinte, false, Some((&t.badge, t.teinte)));
        let sous = egui::RichText::new(insecables(t.sous_titre)).size(13.0).color(erp::TEXTE_2);
        ui.add(egui::Label::new(sous).wrap().selectable(false));
        if !t.legende.is_empty() {
            legende(ui, &t.legende);
        }
        ui.add_space(4.0);
        let cle = egui::Id::unique(("compta-sommaire-rangees", vue));
        cible = if c.etroite { en_rangees(ui, &t, cle) } else { en_tableau(ui, &t, cle) };
        ui.add_space(6.0);
        for n in &t.notes {
            let note = egui::RichText::new(insecables(n)).size(12.5).color(erp::TEXTE_2);
            ui.add(egui::Label::new(note).wrap().selectable(false));
        }
    });
    let ctx = ui.ctx().clone();
    match cible {
        Some(Cible::Fournisseurs { mois, tiers }) => {
            super::aller(&ctx, Aller { vue: VueCompta::Fournisseurs, mois, tiers, ..Aller::default() });
        }
        Some(Cible::Clients { mois }) => {
            super::aller(&ctx, Aller { vue: VueCompta::Clients, mois: Some(mois), ..Aller::default() });
        }
        Some(Cible::Chantier(ch)) => {
            let avant = super::choix(&ctx);
            super::choisir(&ctx, Choix { vue: VueCompta::Bord, filtres: Filtres { chantier: ch, ..avant.filtres } });
        }
        None => {}
    }
}

/// La légende des barres : un carré de chaque couleur, son mot.
fn legende(ui: &mut egui::Ui, l: &[(Color32, &str)]) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for (couleur, mot) in l {
            let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
            ui.painter().rect_filled(r, 3.0, *couleur);
            ui.label(egui::RichText::new(*mot).size(12.5).color(erp::TEXTE_2));
            ui.add_space(8.0);
        }
    });
}

// ---- Le dessin du tableau -------------------------------------------------------------------------------------------

/// La hauteur d'une rangée, de la rangée TOTAL, et celle d'une rangée de deux lignes (fenêtre étroite).
const HAUTEUR: f32 = 34.0;
const HAUTEUR_ETROITE: f32 = 50.0;
/// La marge d'une case, de chaque côté.
const MARGE: f32 = 10.0;
/// Le libellé d'une rangée ne descend pas sous cette largeur ; sinon, la fenêtre étroite.
const LIBELLE_MIN: f32 = 150.0;

fn police() -> FontId {
    FontId::proportional(13.5)
}

fn police_forte() -> FontId {
    FontId::new(13.5, FontFamily::Name(SEMI.into()))
}

fn police_d_entete() -> FontId {
    FontId::new(10.5, FontFamily::Name(SEMI.into()))
}

/// Un texte de l'en-tête : en capitales, ses lettres un peu espacées, comme les tableaux des cartes.
fn texte_d_entete(p: &egui::Painter, texte: &str, couleur: Color32) -> Arc<egui::Galley> {
    let format = egui::TextFormat {
        font_id: police_d_entete(),
        color: couleur,
        extra_letter_spacing: 0.6,
        ..Default::default()
    };
    p.layout_job(egui::text::LayoutJob::single_section(texte.to_uppercase(), format))
}

/// La largeur d'un titre de l'en-tête.
pub(super) fn largeur_d_entete(p: &egui::Painter, titre: &str) -> f32 {
    texte_d_entete(p, titre, Color32::WHITE).size().x
}

/// La largeur du plus large de ces textes, dans cette police.
pub(super) fn plus_large(p: &egui::Painter, textes: &[String], police: FontId) -> f32 {
    textes.iter().map(|t| p.layout_no_wrap(t.clone(), police.clone(), Color32::WHITE).size().x).fold(0.0, f32::max)
}

/// Le jeu qu'on laisse à une case mesurée : à la largeur exacte de son texte, l'arrondi la coupait d'une ellipse.
pub(super) const JEU: f32 = 6.0;

/// Un texte sur une ligne, coupé d'une ellipse s'il dépasse `largeur`.
fn sur_une_ligne(p: &egui::Painter, texte: &str, police: FontId, couleur: Color32, largeur: f32) -> Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(texte.to_string(), police, couleur);
    job.wrap = egui::text::TextWrapping::truncate_at_width(largeur.max(10.0));
    p.layout_job(job)
}

fn encre(e: Encre, fort: bool) -> Color32 {
    match e {
        Encre::Negative => erp::ECART_MAUVAIS,
        Encre::Discrete => erp::TEXTE_3,
        Encre::Normale if fort => erp::TITRE,
        Encre::Normale => erp::TEXTE,
    }
}

/// Les colonnes d'un tableau : leur abscisse de gauche et leur largeur, puis la place des barres (0 : pas de barres).
/// `None` : elles ne tiennent pas dans `largeur` (la fenêtre étroite les montre en rangées).
fn disposer(
    p: &egui::Painter,
    colonnes: &[Colonne],
    candidats: &[Vec<String>],
    largeur: f32,
    avec_barres: bool,
) -> Option<(Vec<(f32, f32)>, f32)> {
    let mut l: Vec<f32> = colonnes
        .iter()
        .enumerate()
        .map(|(k, col)| {
            if k == 0 {
                return 0.0;
            }
            let entete = largeur_d_entete(p, &col.titre);
            let valeur = candidats.get(k).map_or(0.0, |l| plus_large(p, l, police_forte()));
            // Quelques points de jeu : une case mesurée au point près se coupait d'une ellipse à l'arrondi.
            entete.max(valeur) + 2.0 * MARGE + JEU
        })
        .collect();
    let chiffres: f32 = l.iter().sum();
    let barres = if avec_barres { (largeur * 0.13).clamp(90.0, 170.0) } else { 0.0 };
    let (barres, libelle) = if largeur - chiffres - barres >= LIBELLE_MIN {
        (barres, largeur - chiffres - barres)
    } else if largeur - chiffres >= LIBELLE_MIN {
        (0.0, largeur - chiffres)
    } else {
        return None;
    };
    // Le libellé prend ce qui reste, mais pas trop : au-delà de 340 points, le surplus va aux barres (ou se répartit).
    let libelle_max = 340.0f32.max(LIBELLE_MIN);
    let (libelle, barres) = match (libelle > libelle_max, barres > 0.0) {
        (true, true) => (libelle_max, barres + libelle - libelle_max),
        (true, false) => (libelle_max, 0.0),
        _ => (libelle, barres),
    };
    if let Some(x) = l.first_mut() {
        *x = libelle;
    }
    let mut x = 0.0;
    let mut places = Vec::with_capacity(l.len());
    for w in &l {
        places.push((x, *w));
        x += w;
    }
    // Ce qui reste au bout (sans barres, ou un libellé borné) va aux colonnes des chiffres : le tableau prend toute la
    // largeur.
    let surplus = largeur - x - barres;
    if surplus > 0.5 && places.len() > 1 {
        let n = (places.len() - 1) as f32;
        let mut decale = 0.0;
        for (k, place) in places.iter_mut().enumerate() {
            place.0 += decale;
            if k > 0 {
                place.1 += surplus / n;
                decale += surplus / n;
            }
        }
    }
    Some((places, barres))
}

/// Le bandeau de l'en-tête, peint à `y` : marine, ses groupes au-dessus de leurs colonnes (un filet pâle dessous), les
/// titres des colonnes en petites capitales blanches.
pub(super) fn peindre_l_entete(
    p: &egui::Painter,
    gauche: f32,
    y: f32,
    largeur: f32,
    colonnes: &[Colonne],
    places: &[(f32, f32)],
) -> f32 {
    let groupes = colonnes.iter().any(|c| !c.groupe.is_empty());
    let h = if groupes { 46.0 } else { 30.0 };
    let bandeau = Rect::from_min_size(pos2(gauche, y), vec2(largeur, h));
    p.rect_filled(bandeau, 6.0, BANDEAU);
    let y_titres = if groupes { y + 26.0 } else { y + 9.0 };
    if groupes {
        let mut k = 0;
        while k < colonnes.len() {
            let g = colonnes[k].groupe;
            let mut fin = k + 1;
            while fin < colonnes.len() && colonnes[fin].groupe == g {
                fin += 1;
            }
            if !g.is_empty() {
                let (x0, x1) = (gauche + places[k].0 + 6.0, gauche + places[fin - 1].0 + places[fin - 1].1 - 6.0);
                let galee = texte_d_entete(p, g, Color32::WHITE);
                let x = ((x0 + x1) / 2.0 - galee.size().x / 2.0).max(x0);
                p.galley(pos2(x, y + 6.0), galee, Color32::WHITE);
                p.hline(x0..=x1, y + 21.5, Stroke::new(1.0, Color32::from_white_alpha(70)));
            }
            k = fin;
        }
    }
    for (col, (x, w)) in colonnes.iter().zip(places) {
        let galee = texte_d_entete(p, &col.titre, Color32::from_white_alpha(235));
        let gx = if col.droite { gauche + x + w - MARGE - galee.size().x } else { gauche + x + MARGE };
        p.galley(pos2(gx, y_titres), galee, Color32::WHITE);
    }
    h
}

/// Les cases d'une rangée, peintes dans `r`.
fn peindre_les_cases(p: &egui::Painter, r: Rect, rangee: &Rangee, places: &[(f32, f32)], fort: bool) {
    for (k, (case, (x, w))) in rangee.cases.iter().zip(places).enumerate() {
        let largeur = w - 2.0 * MARGE;
        let (police, couleur) = if k == 0 {
            let couleur = if rangee.cible.is_some() && !fort { erp::PRIMAIRE } else { erp::TITRE };
            (police_forte(), couleur)
        } else {
            (if fort { police_forte() } else { police() }, encre(case.encre, fort))
        };
        let g = sur_une_ligne(p, &case.texte, police, couleur, largeur);
        let gx = if k == 0 { r.left() + x + MARGE } else { r.left() + x + w - MARGE - g.size().x };
        p.galley(pos2(gx, r.center().y - g.size().y / 2.0), g, couleur);
    }
}

/// Les barres d'une rangée, dans leur colonne : l'une sous l'autre, centrées sur la rangée.
fn peindre_les_barres(p: &egui::Painter, place: Rect, barres: &[(f32, Color32)]) {
    let (epaisseur, ecart) = (7.0, 3.0);
    let n = barres.len() as f32;
    let haut = place.center().y - (n * epaisseur + (n - 1.0).max(0.0) * ecart) / 2.0;
    for (k, (part, couleur)) in barres.iter().enumerate() {
        if *part <= 0.0 {
            continue;
        }
        let y = haut + k as f32 * (epaisseur + ecart);
        let l = (place.width() * part).max(2.0);
        let r = Rect::from_min_size(pos2(place.left(), y), vec2(l, epaisseur));
        p.rect_filled(r, CornerRadius { nw: 0, sw: 0, ne: 3, se: 3 }, *couleur);
    }
}

/// Le tableau, en colonnes : l'en-tête, les rangées à l'écran seulement, le TOTAL. L'en-tête et le TOTAL restent en
/// vue quand le tableau défile. Rend ce qu'un clic demande.
fn en_tableau(ui: &mut egui::Ui, t: &Tableau, cle: egui::Id) -> Option<Cible> {
    let largeur = ui.available_width();
    let p = ui.painter().clone();
    let avec_barres = t.rangees.iter().any(|r| !r.barres.is_empty());
    let Some((places, l_barres)) = disposer(&p, &t.colonnes, &t.candidats, largeur, avec_barres) else {
        return en_rangees(ui, t, cle);
    };
    let groupes = t.colonnes.iter().any(|c| !c.groupe.is_empty());
    let h_entete = if groupes { 46.0 } else { 30.0 };
    let n = t.rangees.len();
    let (zone, _) = ui.allocate_exact_size(vec2(largeur, h_entete + (n as f32 + 1.0) * HAUTEUR), Sense::hover());
    let corps = Rect::from_min_max(pos2(zone.left(), zone.top() + h_entete), zone.max);
    let ecran = ui.clip_rect();
    let x_barres = zone.left() + places.last().map_or(0.0, |(x, w)| x + w) + 4.0;
    let mut cible = None;
    // Où l'en-tête et le TOTAL se peignent : à leur place, ou contre le haut et le bas de l'écran tant que le tableau
    // continue au-delà. Une rangée qu'ils couvrent ne répond pas à la souris (le clic irait à une rangée cachée).
    let naturel = corps.top() + n as f32 * HAUTEUR;
    let y_entete = zone.top().max(ecran.top()).min(naturel - h_entete);
    let y_total = naturel.min(ecran.bottom() - HAUTEUR).max(corps.top());
    let (couvert_haut, couvert_bas) = (y_entete + h_entete, y_total);
    // Les rangées à l'écran.
    let premiere = (((ecran.top() - corps.top()) / HAUTEUR).floor().max(0.0) as usize).min(n);
    let derniere = (((ecran.bottom() - corps.top()) / HAUTEUR).ceil().max(0.0) as usize).min(n);
    for k in premiere..derniere {
        let rangee = &t.rangees[k];
        let r = Rect::from_min_size(pos2(zone.left(), corps.top() + k as f32 * HAUTEUR), vec2(largeur, HAUTEUR));
        let visible =
            Rect::from_min_max(pos2(r.left(), r.top().max(couvert_haut)), pos2(r.right(), r.bottom().min(couvert_bas)));
        let sens = if visible.height() >= HAUTEUR / 2.0 { Sense::click() } else { Sense::hover() };
        let rep = ui.interact(visible, cle.with(k), sens);
        if rep.hovered() {
            p.rect_filled(r.shrink2(vec2(0.0, 1.0)), 6.0, erp::SURVOL);
        }
        // Les colonnes qui mènent ailleurs (les revenus d'un mois) : leur propre lavis, leur propre bulle.
        let sous = rangee.cible_des_colonnes.as_ref().and_then(|(cols, c, aide)| {
            let x0 = zone.left() + places.get(cols.start)?.0;
            let fin = places.get(cols.end - 1)?;
            let place = Rect::from_min_max(pos2(x0, r.top()), pos2(zone.left() + fin.0 + fin.1, r.bottom()));
            Some((place, c, aide))
        });
        let au_dessus_des_colonnes =
            sous.as_ref().is_some_and(|(place, _, _)| rep.hovered() && ui.rect_contains_pointer(*place));
        if let (true, Some((place, _, _))) = (au_dessus_des_colonnes, &sous) {
            p.rect_filled(place.shrink2(vec2(2.0, 3.0)), 6.0, REVENUS.gamma_multiply(0.12));
        }
        peindre_les_cases(&p, r, rangee, &places, false);
        if l_barres > 0.0 {
            let place = Rect::from_min_size(pos2(x_barres, r.top()), vec2(l_barres - 12.0, HAUTEUR));
            peindre_les_barres(&p, place, &rangee.barres);
        }
        if k + 1 < n {
            p.hline(r.x_range(), r.bottom() - 0.5, Stroke::new(1.0, erp::FILET_DE_CARTE));
        }
        let (aide, la_cible) = match (&sous, au_dessus_des_colonnes) {
            (Some((_, c, aide)), true) => (aide.as_str(), Some((*c).clone())),
            _ => (rangee.aide.as_str(), rangee.cible.clone()),
        };
        let rep = if la_cible.is_some() { rep.on_hover_cursor(egui::CursorIcon::PointingHand) } else { rep };
        let rep = if aide.is_empty() { rep } else { rep.on_hover_text(aide) };
        if rep.clicked() {
            cible = la_cible;
        }
    }
    // Le TOTAL, puis l'en-tête, par-dessus les rangées.
    let r_total = Rect::from_min_size(pos2(zone.left(), y_total), vec2(largeur, HAUTEUR));
    p.rect_filled(r_total, CornerRadius { nw: 0, ne: 0, sw: 6, se: 6 }, FOND_DU_TOTAL);
    p.hline(r_total.x_range(), r_total.top() + 0.75, Stroke::new(1.5, FILET_FORT));
    peindre_les_cases(&p, r_total, &t.total, &places, true);
    peindre_l_entete(&p, zone.left(), y_entete, largeur, &t.colonnes, &places);
    cible
}

/// Le tableau en rangées de deux lignes (une fenêtre étroite) : un en-tête de deux titres (le nom, et ce que dit le
/// chiffre de droite), puis chaque rangée — le nom et le chiffre principal, puis les autres chiffres ; le TOTAL au
/// pied, en vue tant que la liste continue dessous. Rend ce qu'un clic demande.
fn en_rangees(ui: &mut egui::Ui, t: &Tableau, cle: egui::Id) -> Option<Cible> {
    let largeur = ui.available_width();
    let p = ui.painter().clone();
    let deux = [
        Colonne { groupe: "", titre: t.colonnes.first().map_or(String::new(), |c| c.titre.clone()), droite: false },
        Colonne { groupe: "", titre: t.principal.to_string(), droite: true },
    ];
    let (entete, _) = ui.allocate_exact_size(vec2(largeur, 30.0), Sense::hover());
    let moities = [(0.0, largeur / 2.0), (largeur / 2.0, largeur / 2.0)];
    peindre_l_entete(&p, entete.left(), entete.top(), largeur, &deux, &moities);
    let n = t.rangees.len();
    let (zone, _) = ui.allocate_exact_size(vec2(largeur, (n as f32 + 1.0) * HAUTEUR_ETROITE), Sense::hover());
    let ecran = ui.clip_rect();
    let naturel = zone.top() + n as f32 * HAUTEUR_ETROITE;
    let y_total = naturel.min(ecran.bottom() - HAUTEUR_ETROITE).max(zone.top());
    let mut cible = None;
    let dessiner = |r: Rect, rangee: &Rangee, total: bool| {
        let (haut, bas) = (r.top() + 7.0, r.top() + 28.0);
        let principal = &rangee.principal;
        let g_droite = p.layout_no_wrap(principal.texte.clone(), police_forte(), encre(principal.encre, true));
        let l_nom = largeur - g_droite.size().x - 3.0 * MARGE;
        let couleur_nom = if rangee.cible.is_some() && !total { erp::PRIMAIRE } else { erp::TITRE };
        let nom = rangee.cases.first().map_or("", |c| c.texte.as_str());
        let g_nom = sur_une_ligne(&p, nom, police_forte(), couleur_nom, l_nom);
        p.galley(pos2(r.left() + MARGE, haut), g_nom, couleur_nom);
        p.galley(pos2(r.right() - MARGE - g_droite.size().x, haut), g_droite, erp::TITRE);
        let g_reste = sur_une_ligne(&p, &rangee.reste, FontId::proportional(12.5), erp::TEXTE_2, largeur - 2.0 * MARGE);
        p.galley(pos2(r.left() + MARGE, bas), g_reste, erp::TEXTE_2);
    };
    let premiere = (((ecran.top() - zone.top()) / HAUTEUR_ETROITE).floor().max(0.0) as usize).min(n);
    let derniere = (((ecran.bottom() - zone.top()) / HAUTEUR_ETROITE).ceil().max(0.0) as usize).min(n);
    for k in premiere..derniere {
        let rangee = &t.rangees[k];
        let r = Rect::from_min_size(
            pos2(zone.left(), zone.top() + k as f32 * HAUTEUR_ETROITE),
            vec2(largeur, HAUTEUR_ETROITE),
        );
        // Sous le TOTAL qui reste en vue, une rangée ne répond pas à la souris.
        let visible = Rect::from_min_max(r.min, pos2(r.right(), r.bottom().min(y_total)));
        let sens = if visible.height() >= HAUTEUR_ETROITE / 2.0 { Sense::click() } else { Sense::hover() };
        let rep = ui.interact(visible, cle.with(("etroite", k)), sens);
        if rep.hovered() {
            p.rect_filled(r.shrink2(vec2(0.0, 1.0)), 6.0, erp::SURVOL);
        }
        dessiner(r, rangee, false);
        if k + 1 < n {
            p.hline(r.x_range(), r.bottom() - 0.5, Stroke::new(1.0, erp::FILET_DE_CARTE));
        }
        let bulle =
            if rangee.aide.is_empty() { rangee.reste.clone() } else { format!("{}\n{}", rangee.reste, rangee.aide) };
        let rep = if rangee.cible.is_some() { rep.on_hover_cursor(egui::CursorIcon::PointingHand) } else { rep };
        if rep.on_hover_text(bulle).clicked() {
            cible = rangee.cible.clone();
        }
    }
    let r_total = Rect::from_min_size(pos2(zone.left(), y_total), vec2(largeur, HAUTEUR_ETROITE));
    p.rect_filled(r_total, 6.0, FOND_DU_TOTAL);
    p.hline(r_total.x_range(), r_total.top() + 0.75, Stroke::new(1.5, FILET_FORT));
    dessiner(r_total, &t.total, true);
    cible
}

#[cfg(test)]
mod essais {
    use super::*;

    #[test]
    fn les_mois_et_les_longueurs() {
        assert_eq!(mois_dit("2026-10"), "Octobre 2026");
        assert_eq!(mois_dit("2026-08"), "Août 2026");
        assert_eq!(mois_dit("pas un mois"), "pas un mois");
        assert_eq!(longueur(50, 200), 0.25);
        assert_eq!(longueur(-5, 200), 0.0);
        assert_eq!(longueur(300, 200), 1.0, "jamais plus que la colonne");
        assert_eq!(pluriel(1, "facture", "factures"), "1\u{a0}facture");
        assert_eq!(pluriel(0, "facture", "factures"), "0\u{a0}facture");
        assert_eq!(pluriel(3, "facture", "factures"), "3\u{a0}factures");
    }

    #[test]
    fn les_textes_qui_peuvent_etre_les_plus_larges() {
        // Les plus longs, et ceux qui ont jusqu'à deux caractères de moins (« 88 094,72 $ » est plus large que
        // « 14 792,43 $ » : la largeur se mesure sur eux tous) ; une seule fois chacun.
        let textes = ["1", "12345", "1234", "123", "12", "12345"];
        assert_eq!(candidats(textes.iter().copied()), ["12345", "1234", "123"]);
        assert!(candidats(std::iter::empty::<&str>()).is_empty());
        // Seize au plus.
        let beaucoup: Vec<String> = (100..200).map(|n| n.to_string()).collect();
        assert_eq!(candidats(beaucoup.iter().map(String::as_str)).len(), 16);
    }
}
