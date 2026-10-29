//! Le planning : ce que montrent le calendrier, le Gantt et le Kanban du tableau de bord (le thème « Constructo AI ») —
//! l'entreprise, lue dans Constructo AI en lecture seule (`entreprise.rs`, `entreprise.json`), et la maison (ses
//! échéances, ses rappels, ses anniversaires, son école : `activite\*.js`, `rappels.json`). Une seule forme pour les
//! trois vues : les données la remplissent (`Centre`, d'après ses fichiers), les vues la dessinent (`natif\`).
//!
//! C'est le contrat de l'équipe du planning (le lot 12) : ses types ne changent qu'ensemble, d'un commun accord. Une
//! date s'écrit « AAAA-MM-JJ », une heure « HH:MM » ; chaque chose garde sa `source`, ce qu'Eve en dit à Claude quand
//! la personne lui en parle (« En parler à Eve ») — une demande, jamais une écriture : déplacer une carte ou une tâche
//! passe par Claude, et par l'accord de la personne là où les accords valent.

use std::collections::{HashMap, HashSet};

use eve::parole::normaliser;
use serde::{Deserialize, Serialize};

use crate::centre::{date_du_numero, numero_du_jour};
use crate::entreprise::{self, BonDeTravail, Entreprise, FactureOuverte, Operation, Projet, TableauDuKanban};

/// D'où vient une chose : le filtre « Tout · Entreprise · Famille » du tableau de bord (`vue_du_tableau`) vaut pour
/// le planning aussi.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Cote {
    Entreprise,
    Famille,
}

/// Ce qu'est un événement du calendrier : sa sorte lui donne sa teinte et son icône.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Sorte {
    /// Un rendez-vous (l'agenda de l'ERP, l'agenda de la maison).
    RendezVous,
    /// Le début ou la fin d'un chantier.
    Chantier,
    /// Une opération d'un chantier (un bon de travail, une tâche du suivi de production).
    Operation,
    /// Une note du calendrier de l'ERP.
    Note,
    /// Une échéance (fiscale, de l'école, d'un document).
    Echeance,
    /// Une facture qui vient à échéance, ou qui l'a passée.
    Facture,
    /// Une livraison ou une commande attendue.
    Livraison,
    /// Un rappel d'Eve (`rappels.json`).
    Rappel,
    Anniversaire,
    /// L'école : un congé, une journée pédagogique.
    Ecole,
    Autre,
}

/// Un événement du calendrier : un jour (ou plusieurs, `fin` comprise), peut-être une heure.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Evenement {
    /// Son nom dans le planning, unique et le même d'une lecture à l'autre — sa sorte, son jour, son heure et son titre
    /// ([`evenement`], puis [`ranger`]) : le calendrier y tient ce qu'on en a ouvert quand une relecture change l'ordre.
    pub id: String,
    pub debut: String,
    pub fin: Option<String>,
    pub heure: Option<String>,
    pub titre: String,
    pub detail: String,
    pub sorte: Sorte,
    pub cote: Cote,
    /// Ce qu'Eve en dit à Claude quand on lui en parle (« la facture F-1042 de Toitures Exemple, échue le 2 novembre »).
    pub source: String,
}

/// Une tâche du Gantt : un chantier (sans parent) ou l'une de ses opérations (son chantier pour parent).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tache {
    pub id: String,
    pub parent: Option<String>,
    pub titre: String,
    pub debut: String,
    pub fin: String,
    /// De 0 à 1, si l'ERP le dit.
    pub avancement: Option<f32>,
    /// Le statut de l'ERP, tel quel (« En cours », « Termine »…).
    pub statut: String,
    /// Les dates de référence (la « baseline » de l'ERP), pour voir un retard.
    pub prevu: Option<(String, String)>,
    pub responsables: Vec<String>,
    pub source: String,
}

/// Une dépendance entre deux tâches du Gantt : `vers` attend `de` (« FS » : fin à début ; « SS », « FF », « SF »), de
/// tant de jours (`decalage`, négatif : en avance).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Dependance {
    pub de: String,
    pub vers: String,
    pub sorte: String,
    pub decalage: i32,
}

/// Un tableau du Kanban : un « board » du suivi de l'ERP (les chantiers, les soumissions, les bons de travail, les
/// ventes…), ses colonnes dans l'ordre de l'ERP (`suivi_colonnes`), et sa règle (« Statut géré via le Bon de travail »).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tableau {
    pub cle: String,
    pub titre: String,
    pub colonnes: Vec<Colonne>,
    pub garde: Option<String>,
}

/// Une colonne du Kanban : son statut (celui de l'ERP), son titre lisible, combien de cartes l'ERP y compte (`total`,
/// d'après `suivi_etat_kanban`), et celles qu'Eve en a lues.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Colonne {
    pub statut: String,
    pub titre: String,
    pub total: usize,
    pub cartes: Vec<Carte>,
}

/// Une carte du Kanban : un chantier, une soumission, un bon de travail…
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Carte {
    pub id: String,
    pub titre: String,
    pub detail: String,
    pub montant: Option<f64>,
    pub echeance: Option<String>,
    pub responsables: Vec<String>,
    pub source: String,
}

/// Le planning entier, d'un coup : ce que dessinent les trois vues.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Planning {
    pub evenements: Vec<Evenement>,
    pub taches: Vec<Tache>,
    pub dependances: Vec<Dependance>,
    pub tableaux: Vec<Tableau>,
    /// Quand l'entreprise en a été lue (« 2026-10-28 14:02 ») ; vide : pas encore (ou pas reliée).
    pub lu: String,
}

// ---- Le remplir : la fenêtre, les jours, les statuts, l'entreprise ------------------------------------------------
//
// Ce que la dernière lecture de l'ERP a gardé (`entreprise.json`, `entreprise::Entreprise`) devient ici les événements
// de l'entreprise, les tâches et les dépendances du Gantt, les tableaux du Kanban ; la maison s'y ajoute dans
// `Centre::planning`, qui tient les fichiers de la maison.

/// La fenêtre du planning autour d'`aujourdhui` (« AAAA-MM-JJ ») : du premier jour du mois d'avant au dernier jour du
/// deuxième mois après — un 28 octobre, du 1er septembre au 31 décembre. Les notes du calendrier de l'ERP s'y lisent,
/// une opération finie avant elle ne s'y montre plus ; le calendrier y tourne ses pages. Une date illisible : deux
/// textes vides.
pub fn fenetre(aujourdhui: &str) -> (String, String) {
    let (Some(_), Some(a), Some(m)) = (
        numero_du_jour(aujourdhui),
        aujourdhui.get(..4).and_then(|a| a.parse::<i64>().ok()),
        aujourdhui.get(5..7).and_then(|m| m.parse::<i64>().ok()),
    ) else {
        return (String::new(), String::new());
    };
    // Le premier jour du mois de rang `k`, les mois comptés depuis l'an zéro.
    let premier = |k: i64| format!("{:04}-{:02}-01", k.div_euclid(12), k.rem_euclid(12) + 1);
    let k = a * 12 + m - 1;
    let fin = numero_du_jour(&premier(k + 3)).map(|n| date_du_numero(n - 1)).unwrap_or_default();
    (premier(k - 1), fin)
}

/// « mardi 27 octobre », « dimanche 1er novembre » ; une date illisible, telle quelle.
fn jour_long(date: &str) -> String {
    if numero_du_jour(date).is_none() {
        return date.to_string();
    }
    crate::ambiance::date_longue(date).to_lowercase().replacen(" 1 ", " 1er ", 1)
}

/// « le mardi 27 octobre » : un jour comme Eve le dit à Claude.
pub fn jour_dit(date: &str) -> String {
    format!("le {}", jour_long(date))
}

/// « le mardi 27 octobre » pour un seul jour, sinon « du mardi 27 octobre au jeudi 29 octobre ».
fn du_au(debut: &str, fin: &str) -> String {
    if debut == fin { jour_dit(debut) } else { format!("du {} au {}", jour_long(debut), jour_long(fin)) }
}

/// Les mots de l'ERP, qui les écrit sans accents — ses statuts, les titres de ses tableaux et de leurs colonnes, leurs
/// règles (« Statut gere via le Bon de travail ») —, et leurs accents.
const ACCENTS: &[(&str, &str)] = &[
    ("accepte", "accepté"),
    ("acceptee", "acceptée"),
    ("annule", "annulé"),
    ("annulee", "annulée"),
    ("approuve", "approuvé"),
    ("approuvee", "approuvée"),
    ("arrete", "arrêté"),
    ("bloque", "bloqué"),
    ("bloquee", "bloquée"),
    ("complete", "complété"),
    ("completee", "complétée"),
    ("cree", "créé"),
    ("creee", "créée"),
    ("degager", "dégager"),
    ("echu", "échu"),
    ("echue", "échue"),
    ("engagee", "engagée"),
    ("envoye", "envoyé"),
    ("envoyee", "envoyée"),
    ("etape", "étape"),
    ("etapes", "étapes"),
    ("etat", "état"),
    ("ferme", "fermé"),
    ("fermee", "fermée"),
    ("gagne", "gagné"),
    ("gagnee", "gagnée"),
    ("gere", "géré"),
    ("geree", "gérée"),
    ("geres", "gérés"),
    ("livre", "livré"),
    ("livree", "livrée"),
    ("modifie", "modifié"),
    ("negociation", "négociation"),
    ("operation", "opération"),
    ("operations", "opérations"),
    ("paye", "payé"),
    ("payee", "payée"),
    ("planifie", "planifié"),
    ("planifiee", "planifiée"),
    ("planifies", "planifiés"),
    ("pret", "prêt"),
    ("prete", "prête"),
    ("prevu", "prévu"),
    ("prevue", "prévue"),
    ("realise", "réalisé"),
    ("recu", "reçu"),
    ("recue", "reçue"),
    ("refuse", "refusé"),
    ("refusee", "refusée"),
    ("termine", "terminé"),
    ("terminee", "terminée"),
    ("terminees", "terminées"),
    ("termines", "terminés"),
    ("valide", "validé"),
    ("validee", "validée"),
    ("verifie", "vérifié"),
];

/// Ceux qui ne prennent leurs accents que dans un statut : ailleurs, ce sont d'autres mots (« Bon de commande », « la
/// facture », « le devis a été envoyé », « il confirme »).
const ACCENTS_D_UN_STATUT: &[(&str, &str)] =
    &[("a", "à"), ("commande", "commandé"), ("confirme", "confirmé"), ("expire", "expiré"), ("facture", "facturé")];

/// Un texte de l'ERP tel qu'il se lit à l'écran — un titre, une règle d'un tableau (`garde`) : l'ERP les écrit sans
/// accents, parfois en capitales liées (« Termine », « EN_ATTENTE », « Statut gere via le Bon de travail ») ; ses mots
/// connus prennent leurs accents (« Terminé », « En attente », « Statut géré… »), un « A » en tête devient « À » (« À
/// planifier »), et ce qui était tout en capitales passe en casse de phrase. Ce qui va au chef ou à Claude garde le
/// texte de l'ERP, tel quel.
pub fn en_francais(texte: &str) -> String {
    accentuer(texte, false)
}

/// Un statut de l'ERP tel qu'il se lit : « EN_COURS » → « En cours », « Termine » → « Terminé », « PARTIELLEMENT_PAYEE
/// » → « Partiellement payée », « A_DEGAGER » → « À dégager » — comme [`en_francais`], ses `_` en espaces, la
/// première lettre en capitale, et les mots d'un statut (`ACCENTS_D_UN_STATUT` : « COMMANDE », « Commandé »). Vide :
/// « Sans statut ».
pub fn titre_de_statut(statut: &str) -> String {
    let n = normaliser(statut);
    if n.is_empty() || n == "sans statut" {
        return "Sans statut".into();
    }
    let t = accentuer(statut, true);
    let mut c = t.chars();
    c.next().map(|p| p.to_uppercase().chain(c).collect()).unwrap_or_default()
}

/// Les accents d'un texte de l'ERP, mot à mot, sa ponctuation gardée ; `statut` : un statut ([`titre_de_statut`]).
fn accentuer(texte: &str, statut: bool) -> String {
    let t = texte.trim();
    let capitales = t.chars().any(char::is_alphabetic) && !t.chars().any(char::is_lowercase);
    let t = if capitales { t.to_lowercase() } else { t.to_string() };
    let t = if capitales || statut { t.replace('_', " ") } else { t };
    let du_statut: &[(&str, &str)] = if statut { ACCENTS_D_UN_STATUT } else { &[] };
    let mots: Vec<String> = t
        .split(' ')
        .filter(|m| !m.is_empty())
        .enumerate()
        .map(|(k, mot)| {
            let debut = mot.find(char::is_alphanumeric).unwrap_or(mot.len());
            let fin = mot
                .rfind(char::is_alphanumeric)
                .map_or(debut, |i| i + mot[i..].chars().next().map_or(1, char::len_utf8));
            let (avant, coeur, apres) = (&mot[..debut], &mot[debut..fin], &mot[fin..]);
            let bas = coeur.to_lowercase();
            let accentue = if k == 0 && bas == "a" {
                Some("à")
            } else {
                ACCENTS.iter().chain(du_statut).find(|(sans, _)| *sans == bas).map(|(_, avec)| *avec)
            };
            let coeur = match accentue {
                Some(avec) if coeur.starts_with(char::is_uppercase) => {
                    let mut c = avec.chars();
                    c.next().map_or(String::new(), |p| p.to_uppercase().collect::<String>() + c.as_str())
                }
                Some(avec) => avec.to_string(),
                None => coeur.to_string(),
            };
            format!("{avant}{coeur}{apres}")
        })
        .collect();
    let phrase = mots.join(" ");
    if capitales {
        let mut c = phrase.chars();
        c.next().map_or(String::new(), |p| p.to_uppercase().collect::<String>() + c.as_str())
    } else {
        phrase
    }
}

/// Le statut d'une tâche du Gantt, dans la forme que l'ERP donne à son propre Gantt — « En attente », « En cours »,
/// « Termine », « Annule », « Suspendu » — ses variantes ramenées à elle (`_PROJ_STATUT_CANON` de `mcp_server.py`,
/// `_OPERATION_STATUT_NORM` de son Gantt ; un bon de travail en brouillon attend, en pause il est suspendu) : les vues
/// y prennent la teinte d'une barre. Vide (ou « (sans statut) », comme l'ERP compte un statut vide) : « En attente »,
/// comme l'ERP pour une opération ; inconnu : tel quel.
pub fn statut_du_gantt(statut: &str) -> String {
    match normaliser(statut).as_str() {
        "" | "sans statut" | "en attente" | "a faire" | "planifie" | "brouillon" => "En attente",
        "en cours" | "actif" | "in progress" => "En cours",
        "termine" | "terminee" | "complete" | "completed" | "fini" => "Termine",
        "annule" | "annulee" | "cancelled" | "canceled" => "Annule",
        "suspendu" | "en pause" | "on hold" => "Suspendu",
        _ => return statut.trim().to_string(),
    }
    .to_string()
}

fn annule(statut: &Option<String>) -> bool {
    statut_du_gantt(statut.as_deref().unwrap_or("")) == "Annule"
}

/// Des morceaux, ceux qui ont quelque chose, liés d'un « · ».
fn joint(morceaux: &[Option<&str>]) -> String {
    morceaux.iter().flatten().map(|m| m.trim()).filter(|m| !m.is_empty()).collect::<Vec<_>>().join(" · ")
}

/// « (a, b) », précédé d'une espace ; rien si les morceaux n'ont rien.
fn entre_parentheses(morceaux: &[Option<&str>]) -> String {
    let t = joint(morceaux).replace(" · ", ", ");
    if t.is_empty() { t } else { format!(" ({t})") }
}

/// Le fournisseur d'une opération, s'il n'est pas l'entreprise elle-même (« Interne ») : le sous-traitant.
fn sous_traitant(o: &Operation) -> Option<&str> {
    o.fournisseur.as_deref().map(str::trim).filter(|f| !f.is_empty() && !f.eq_ignore_ascii_case("interne"))
}

/// L'avancement d'une opération, comme l'ERP le calcule pour son Gantt : ses heures faites sur ses heures prévues, au
/// plus 1 ; finie, 1 ; sans heures prévues, rien.
fn avancement(o: &Operation) -> Option<f32> {
    if statut_du_gantt(o.statut.as_deref().unwrap_or("")) == "Termine" {
        return Some(1.0);
    }
    let prevues = o.heures_prevues.filter(|h| *h > 0.0)?;
    Some((o.heures_reelles.unwrap_or(0.0) / prevues).clamp(0.0, 1.0) as f32)
}

/// Des heures comme on les écrit : « 12 », « 7,5 ».
fn heures(h: f64) -> String {
    let t = format!("{:.1}", (h * 10.0).round() / 10.0);
    t.trim_end_matches(".0").replace('.', ",")
}

/// La clé d'un chantier, dans l'id de sa tâche du Gantt et de sa carte du Kanban (« projet-14 ») : son numéro dans
/// l'ERP ; sans lui (un `entreprise.json` d'avant qu'Eve le garde), son numéro de document, sinon son nom.
fn cle_du_chantier(p: &Projet) -> String {
    p.id.map(|id| id.to_string()).or_else(|| p.numero.clone()).unwrap_or_else(|| p.nom.clone())
}

/// Les chantiers de l'entreprise : en cours, puis à venir (une seule fois chacun).
fn chantiers(e: &Entreprise) -> Vec<&Projet> {
    let mut vus = HashSet::new();
    e.projets
        .iter()
        .flatten()
        .chain(e.projets_en_attente.iter().flatten())
        .filter(|p| p.id.is_none_or(|id| vus.insert(id)))
        .collect()
}

// ---- Ce qu'Eve en dit à Claude (la `source` de chaque chose) -----------------------------------------------------
//
// Assez pour que Claude la retrouve dans l'ERP et sache de quoi la personne parle : sa sorte, son numéro de document et
// son numéro dans l'ERP (celui que prennent ses outils), ses dates, son statut.

/// « le chantier « Agrandissement, 12 rue des Exemples » (PROJ-2026-00014, projet n° 14 de l'ERP, pour Construction
/// Exemple inc.) ».
fn chantier_dit(p: &Projet) -> String {
    let id = p.id.map(|id| format!("projet n° {id} de l'ERP"));
    let client = p.client.as_deref().map(|c| format!("pour {c}"));
    let entre = joint(&[p.numero.as_deref(), id.as_deref(), client.as_deref()]).replace(" · ", ", ");
    if entre.is_empty() {
        format!("le chantier « {} »", p.nom)
    } else {
        format!("le chantier « {} » ({entre})", p.nom)
    }
}

/// « le bon de travail BT-2026-041 « Charpente » (bon de travail n° 41 de l'ERP, du chantier « … ») ».
fn bon_dit(b: &BonDeTravail) -> String {
    let nom = b.nom.as_deref().map(|n| format!(" « {n} »")).unwrap_or_default();
    let numero = b.numero.as_deref().map(|n| format!(" {n}")).unwrap_or_default();
    let chantier = b.projet.as_deref().map(|p| format!(", du chantier « {p} »")).unwrap_or_default();
    format!("le bon de travail{numero}{nom} (bon de travail n° {} de l'ERP{chantier})", b.id)
}

/// « l'opération « Pose des fermes » (opération n° 501 de l'ERP) du bon de travail BT-2026-041 du chantier « … »,
/// confiée à Alex Modèle, 12 h faites sur 24 prévues ».
fn operation_dite(o: &Operation, bt: Option<&BonDeTravail>) -> String {
    let mut s = format!("l'opération « {} » (opération n° {} de l'ERP)", o.nom, o.id);
    if let Some(n) = o.bt_numero.as_deref().or(bt.and_then(|b| b.numero.as_deref())) {
        s.push_str(&format!(" du bon de travail {n}"));
    }
    if let Some(p) = bt.and_then(|b| b.projet.as_deref()) {
        s.push_str(&format!(" du chantier « {p} »"));
    }
    if let Some(r) = o.responsable.as_deref() {
        s.push_str(&format!(", confiée à {r}"));
    }
    if let Some(f) = sous_traitant(o) {
        s.push_str(&format!(", donnée en sous-traitance à {f}"));
    }
    if let Some(p) = o.heures_prevues.filter(|h| *h > 0.0) {
        s.push_str(&format!(", {} h faites sur {} prévues", heures(o.heures_reelles.unwrap_or(0.0)), heures(p)));
    }
    s
}

/// « la facture FACT-2026-00211 de Construction Exemple inc. (facture n° 211 de l'ERP), 28 744 $ à recevoir ».
fn facture_dite(f: &FactureOuverte) -> String {
    let numero = f.numero.as_deref().map(|n| format!(" {n}")).unwrap_or_default();
    let tiers = f.tiers.as_deref().map(|t| format!(" de {t}")).unwrap_or_default();
    let id = f.id.map(|id| format!(" (facture n° {id} de l'ERP)")).unwrap_or_default();
    let sens = if f.sorte == "fournisseur" { "à payer" } else { "à recevoir" };
    let projet = f.projet.as_deref().map(|p| format!(", pour le chantier « {p} »")).unwrap_or_default();
    format!("la facture{numero}{tiers}{id}{projet}, {} {sens}", entreprise::dollars(f.solde))
}

// ---- Le calendrier : les événements de l'entreprise -----------------------------------------------------------------

/// Un événement du calendrier, s'il se place : `quand` — son premier jour (« AAAA-MM-JJ »), son dernier (compris ;
/// seulement s'il vient après), son heure (« HH:MM ») — ; un titre qui dit quelque chose.
pub(crate) fn evenement(
    (debut, fin, heure): (&str, Option<&str>, Option<&str>),
    titre: String,
    detail: String,
    (sorte, cote): (Sorte, Cote),
    source: String,
) -> Option<Evenement> {
    if numero_du_jour(debut).is_none() || titre.trim().is_empty() {
        return None;
    }
    let fin = fin.filter(|f| numero_du_jour(f).is_some() && *f > debut).map(str::to_string);
    let heure = heure.and_then(heure_du_calendrier);
    Some(Evenement {
        id: format!("{sorte:?}·{debut}·{}·{}", heure.as_deref().unwrap_or(""), titre.trim()).to_lowercase(),
        debut: debut.into(),
        fin,
        heure,
        titre,
        detail,
        sorte,
        cote,
        source,
    })
}

/// Une heure comme le contrat l'écrit, « 08:30 », d'après « 8:30 », « 08:30 » ou « 08:30:00 » ; rien si ce n'en est
/// pas une.
fn heure_du_calendrier(h: &str) -> Option<String> {
    let (hh, mm) = h.trim().split_once(':')?;
    let (hh, mm) = (hh.parse::<u32>().ok()?, mm.get(..2)?.parse::<u32>().ok()?);
    (hh < 24 && mm < 60).then(|| format!("{hh:02}:{mm:02}"))
}

/// Les événements de l'entreprise : les notes du calendrier de l'ERP ; le début et la fin de ses chantiers (en cours et
/// à venir) ; ses bons de travail, du début à la fin (sinon à leur échéance, comme le calendrier de l'ERP les place) ;
/// les opérations du suivi de production, s'il a été lu ; les factures qui attendent leur solde, à leur échéance ; les
/// livraisons et les commandes attendues ; les bons d'achat à commander, à leur échéance. Ni ce qui est annulé, ni ce
/// qui n'a pas de date.
fn evenements_de_l_entreprise(e: &Entreprise, aujourdhui: &str) -> Vec<Evenement> {
    let mut ev = Vec::new();
    let de = |sorte| (sorte, Cote::Entreprise);

    for n in e.agenda.iter().flatten() {
        let source = format!("la note du calendrier de l'ERP {} : « {} »", jour_dit(&n.quand), n.texte);
        ev.extend(evenement((&n.quand, None, None), n.texte.clone(), String::new(), de(Sorte::Note), source));
    }
    for p in chantiers(e) {
        let qui = joint(&[p.client.as_deref(), p.ville.as_deref()]);
        if let Some(d) = &p.debut {
            let detail = joint(&[Some("Début du chantier"), Some(&qui)]);
            let source = format!("le début de {}, {}", chantier_dit(p), jour_dit(d));
            ev.extend(evenement((d, None, None), p.nom.clone(), detail, de(Sorte::Chantier), source));
        }
        if let Some(f) = &p.fin {
            let detail = joint(&[Some("Fin du chantier"), Some(&qui)]);
            let source = format!("la fin prévue de {}, {}", chantier_dit(p), jour_dit(f));
            ev.extend(evenement((f, None, None), p.nom.clone(), detail, de(Sorte::Chantier), source));
        }
    }
    let bons: HashMap<u64, &BonDeTravail> = e.bons_travail.iter().flatten().map(|b| (b.id, b)).collect();
    for b in e.bons_travail.iter().flatten().filter(|b| !annule(&b.statut)) {
        let Some(debut) = b.debut.as_deref().or(b.echeance.as_deref()).or(b.fin.as_deref()) else { continue };
        let fin = b.fin.as_deref().or(b.echeance.as_deref()).filter(|f| *f > debut);
        let titre = b.nom.clone().or_else(|| b.numero.clone()).unwrap_or_else(|| format!("Bon de travail {}", b.id));
        let numero = b.numero.as_deref().map(|n| format!("Bon de travail {n}"));
        let detail = joint(&[numero.as_deref(), b.projet.as_deref()]);
        let statut = titre_de_statut(b.statut.as_deref().unwrap_or("")).to_lowercase();
        let source = format!("{}, {}, statut {statut}", bon_dit(b), du_au(debut, fin.unwrap_or(debut)));
        ev.extend(evenement((debut, fin, None), titre, detail, de(Sorte::Operation), source));
    }
    for o in e.operations.iter().flatten() {
        let bt = o.bt_id.and_then(|i| bons.get(&i).copied());
        if annule(&o.statut) || bt.is_some_and(|b| annule(&b.statut)) {
            continue;
        }
        let Some(debut) = o.debut.as_deref().or(o.fin.as_deref()) else { continue };
        let fin = o.fin.as_deref().filter(|f| *f > debut);
        let qui = o.responsable.as_deref().or(sous_traitant(o));
        let detail = joint(&[o.bt_numero.as_deref(), bt.and_then(|b| b.projet.as_deref()), qui]);
        let source = format!("{}, prévue {}", operation_dite(o, bt), du_au(debut, fin.unwrap_or(debut)));
        ev.extend(evenement((debut, fin, None), o.nom.clone(), detail, de(Sorte::Operation), source));
    }
    for f in e.factures_ouvertes.iter().flatten() {
        let Some(d) = f.echeance.as_deref() else { continue };
        let tiers = f.tiers.as_deref().or(f.numero.as_deref()).unwrap_or("une facture");
        let sens = if f.sorte == "fournisseur" { "À payer" } else { "À recevoir" };
        let combien = format!("{sens} : {}", entreprise::dollars(f.solde));
        let detail = joint(&[f.numero.as_deref(), Some(&combien)]);
        let quand = if d < aujourdhui { "échue" } else { "à échéance" };
        let source = format!("{}, {quand} {}", facture_dite(f), jour_dit(d));
        ev.extend(evenement((d, None, None), format!("Facture · {tiers}"), detail, de(Sorte::Facture), source));
    }
    for c in e.commandes.iter().flatten() {
        let Some(d) = c.date.as_deref() else { continue };
        let id = c.id.map(|id| format!("n° {id} de l'ERP"));
        let numero = c.numero.as_deref();
        let chantier = c.projet.as_deref().map(|p| format!(" du chantier « {p} »")).unwrap_or_default();
        match c.sorte.as_str() {
            "livraison" => {
                let titre = format!("Livraison · {}", c.projet.as_deref().or(numero).unwrap_or("un chantier"));
                let detail = joint(&[numero, c.texte.as_deref()]);
                let a =
                    c.heure.as_deref().map(|h| format!(" à {}", crate::centre::heure_parlee(h))).unwrap_or_default();
                let quoi = entre_parentheses(&[numero, c.texte.as_deref(), id.as_deref()]);
                let source = format!("la livraison{quoi}{chantier}, prévue {}{a}", jour_dit(d));
                ev.extend(evenement((d, None, c.heure.as_deref()), titre, detail, de(Sorte::Livraison), source));
            }
            "commande" => {
                let titre = format!("Livraison · {}", c.fournisseur.as_deref().or(numero).unwrap_or("une commande"));
                let bon = numero.map(|n| format!("Bon de commande {n}"));
                let detail = joint(&[bon.as_deref(), c.projet.as_deref()]);
                let de_qui = c.fournisseur.as_deref().map(|f| format!("de {f}"));
                let quoi = entre_parentheses(&[de_qui.as_deref(), id.as_deref()]);
                let source = format!(
                    "le bon de commande {}{quoi}{chantier}, livraison prévue {}",
                    numero.unwrap_or(""),
                    jour_dit(d)
                );
                ev.extend(evenement((d, None, None), titre, detail, de(Sorte::Livraison), source));
            }
            "a_commander" => {
                let titre = format!("À commander · {}", c.texte.as_deref().or(numero).unwrap_or("un achat"));
                let bon = numero.map(|n| format!("Bon d'achat {n}"));
                let detail = joint(&[bon.as_deref(), c.projet.as_deref()]);
                let quoi = entre_parentheses(&[c.texte.as_deref(), id.as_deref()]);
                let source = format!(
                    "le bon d'achat {}{quoi}{chantier}, à commander d'ici {}",
                    numero.unwrap_or(""),
                    jour_dit(d)
                );
                ev.extend(evenement((d, None, None), titre, detail, de(Sorte::Echeance), source));
            }
            _ => {}
        }
    }
    ev
}

/// Les événements dans l'ordre du calendrier : par jour ; ceux de toute la journée d'abord, puis à leur heure. Et
/// chacun son `id` à lui : deux pareils (la même sorte, le même jour, la même heure, le même titre), le second
/// « ·2 », le troisième « ·3 ».
pub fn ranger(evenements: &mut [Evenement]) {
    evenements.sort_by(|a, b| {
        (&a.debut, a.heure.is_some(), &a.heure, &a.titre).cmp(&(&b.debut, b.heure.is_some(), &b.heure, &b.titre))
    });
    let mut vus: HashMap<String, usize> = HashMap::new();
    for e in evenements.iter_mut() {
        let n = vus.entry(e.id.clone()).or_default();
        *n += 1;
        if *n > 1 {
            e.id = format!("{}·{n}", e.id);
        }
    }
}

// ---- Le Gantt : les chantiers et leurs opérations ----------------------------------------------------------------

/// L'id d'une tâche du Gantt d'après la sorte que lui donne l'ERP (`project`, `bt`, `op`) et son numéro — celui de la
/// carte du Kanban qui montre la même chose. Les autres sortes de l'ERP (`devis`, `bc`, `opp`) n'ont pas de tâche ici.
fn id_de_tache(sorte: &str, id: &str) -> Option<String> {
    match sorte {
        "project" => Some(format!("projet-{id}")),
        "bt" => Some(format!("bt-{id}")),
        "op" => Some(format!("op-{id}")),
        _ => None,
    }
}

/// Le premier et le dernier jour d'opérations.
fn etendue(operations: &[&Operation]) -> (Option<String>, Option<String>) {
    let debut = operations.iter().filter_map(|o| o.debut.clone().or_else(|| o.fin.clone())).min();
    let fin = operations.iter().filter_map(|o| o.fin.clone().or_else(|| o.debut.clone())).max();
    (debut, fin)
}

/// Le statut d'un ensemble d'opérations : finies toutes, il est fini ; commencées en partie, en cours ; sinon il
/// attend.
fn statut_d_ensemble(taches: &[Tache]) -> String {
    let fini = |t: &Tache| t.statut == "Termine";
    let statut = if !taches.is_empty() && taches.iter().all(fini) {
        "Termine"
    } else if taches.iter().any(|t| fini(t) || t.statut == "En cours") {
        "En cours"
    } else {
        "En attente"
    };
    statut.into()
}

/// Les tâches du Gantt et leurs dépendances : les chantiers en cours et à venir, et sous chacun ses opérations (le
/// suivi de production, lu à la demande) — un bon de travail dont aucune opération n'est lue s'y montre à leur place ;
/// un bon de travail sans chantier connu tient lieu de chantier aux siennes. Ni les bons de travail annulés ni leurs
/// opérations (l'ERP les écarte de son Gantt), ni les opérations annulées, ni ce qui n'a pas de date. Un chantier
/// commencé sans date de fin dure au moins jusqu'à aujourd'hui ; sans dates, il prend celles de ses opérations. Les
/// tâches vont dans l'ordre où elles se montrent : chaque chantier (par son début), puis les siennes. Les dépendances :
/// celles de l'ERP entre deux tâches qui y sont.
fn gantt(e: &Entreprise, aujourdhui: &str) -> (Vec<Tache>, Vec<Dependance>) {
    let references: HashMap<(String, String), (String, String)> = e
        .references
        .iter()
        .flatten()
        .map(|r| {
            let fin = r.fin.clone().filter(|f| *f >= r.debut).unwrap_or_else(|| r.debut.clone());
            ((r.sorte.clone(), r.id.clone()), (r.debut.clone(), fin))
        })
        .collect();
    let prevu = |sorte: &str, id: u64| references.get(&(sorte.to_string(), id.to_string())).cloned();
    let bons: Vec<&BonDeTravail> = e.bons_travail.iter().flatten().filter(|b| !annule(&b.statut)).collect();
    let annules: HashSet<u64> = e.bons_travail.iter().flatten().filter(|b| annule(&b.statut)).map(|b| b.id).collect();
    // Les opérations de chaque bon de travail, dans l'ordre de l'ERP (le bon, puis le rang de chacune).
    let mut ordre_des_bons: Vec<u64> = Vec::new();
    let mut operations: HashMap<u64, Vec<&Operation>> = HashMap::new();
    for o in e.operations.iter().flatten() {
        let Some(bt) = o.bt_id else { continue };
        if annules.contains(&bt) || annule(&o.statut) || (o.debut.is_none() && o.fin.is_none()) {
            continue;
        }
        if !operations.contains_key(&bt) {
            ordre_des_bons.push(bt);
        }
        operations.entry(bt).or_default().push(o);
    }
    let chantiers: Vec<&Projet> = chantiers(e);
    let ids_des_chantiers: HashSet<u64> = chantiers.iter().filter_map(|p| p.id).collect();

    let tache_d_operation = |o: &Operation, parent: &str, bt: Option<&BonDeTravail>| -> Tache {
        let debut = o.debut.clone().or_else(|| o.fin.clone()).unwrap_or_default();
        let fin = o.fin.clone().filter(|f| *f >= debut).unwrap_or_else(|| debut.clone());
        let responsables =
            [o.responsable.as_deref(), sous_traitant(o)].into_iter().flatten().map(str::to_string).collect();
        Tache {
            id: format!("op-{}", o.id),
            parent: Some(parent.to_string()),
            titre: o.nom.clone(),
            source: format!("{}, prévue {}", operation_dite(o, bt), du_au(&debut, &fin)),
            debut,
            fin,
            avancement: avancement(o),
            statut: statut_du_gantt(o.statut.as_deref().unwrap_or("")),
            prevu: prevu("op", o.id),
            responsables,
        }
    };
    // Un bon de travail : ses dates, sinon celles de ses opérations, sinon son échéance (l'ordre de l'ERP).
    let tache_de_bon = |b: &BonDeTravail, parent: Option<String>, ops: &[&Operation]| -> Option<Tache> {
        let (debut_des_ops, fin_des_ops) = etendue(ops);
        let debut = b.debut.clone().or(debut_des_ops).or_else(|| b.echeance.clone()).or_else(|| b.fin.clone())?;
        let mut fin = b.fin.clone().or_else(|| b.echeance.clone());
        if b.debut.is_none() {
            fin = fin.max(fin_des_ops);
        }
        let fin = fin.filter(|f| *f >= debut).unwrap_or_else(|| debut.clone());
        let statut = b.statut.as_deref().unwrap_or("");
        let echeance = b.echeance.as_deref().map(|e| format!(", à finir {}", jour_dit(e))).unwrap_or_default();
        let source = format!(
            "{}, {}{echeance}, statut {}",
            bon_dit(b),
            du_au(&debut, &fin),
            titre_de_statut(statut).to_lowercase()
        );
        Some(Tache {
            id: format!("bt-{}", b.id),
            parent,
            titre: b.nom.clone().or_else(|| b.numero.clone()).unwrap_or_else(|| format!("Bon de travail {}", b.id)),
            debut,
            fin,
            avancement: None,
            statut: statut_du_gantt(statut),
            prevu: prevu("bt", b.id),
            responsables: Vec::new(),
            source,
        })
    };
    let ranger_les_enfants = |enfants: &mut Vec<Tache>| enfants.sort_by(|a, b| a.debut.cmp(&b.debut));

    // Chaque chantier et les siens.
    let mut groupes: Vec<(Tache, Vec<Tache>)> = Vec::new();
    for p in &chantiers {
        let parent = format!("projet-{}", cle_du_chantier(p));
        let mut enfants: Vec<Tache> = Vec::new();
        for b in bons.iter().filter(|b| p.id.is_some() && b.projet_id == p.id) {
            match operations.get(&b.id) {
                Some(ops) => enfants.extend(ops.iter().map(|o| tache_d_operation(o, &parent, Some(*b)))),
                None => enfants.extend(tache_de_bon(b, Some(parent.clone()), &[])),
            }
        }
        ranger_les_enfants(&mut enfants);
        let premier = enfants.iter().map(|t| t.debut.clone()).min();
        let dernier = enfants.iter().map(|t| t.fin.clone()).max();
        let Some(debut) = p.debut.clone().or(premier) else { continue };
        let statut = statut_du_gantt(p.statut.as_deref().unwrap_or(""));
        let fin = match p.fin.clone().filter(|f| *f >= debut) {
            Some(f) => f,
            // Sans date de fin : jusqu'à son dernier travail, et en cours, au moins jusqu'à aujourd'hui.
            None => {
                let jusqua = (statut == "En cours").then(|| aujourdhui.to_string());
                dernier.max(jusqua).filter(|f| *f >= debut).unwrap_or_else(|| debut.clone())
            }
        };
        let sans_fin = if p.fin.is_none() { ", sans date de fin dans l'ERP" } else { "" };
        let source = format!(
            "{}, {}{sans_fin}, statut {}",
            chantier_dit(p),
            du_au(&debut, &fin),
            titre_de_statut(p.statut.as_deref().unwrap_or("")).to_lowercase()
        );
        let tache = Tache {
            id: parent,
            parent: None,
            titre: p.nom.clone(),
            debut,
            fin,
            avancement: None,
            statut,
            prevu: p.id.and_then(|id| prevu("project", id)),
            responsables: Vec::new(),
            source,
        };
        groupes.push((tache, enfants));
    }
    // Les bons de travail sans chantier connu, avec leurs opérations.
    for b in bons.iter().filter(|b| b.projet_id.is_none_or(|p| !ids_des_chantiers.contains(&p))) {
        let ops = operations.get(&b.id).map(Vec::as_slice).unwrap_or(&[]);
        let Some(tache) = tache_de_bon(b, None, ops) else { continue };
        let mut enfants: Vec<Tache> = ops.iter().map(|o| tache_d_operation(o, &tache.id, Some(*b))).collect();
        ranger_les_enfants(&mut enfants);
        groupes.push((tache, enfants));
    }
    // Les bons de travail que l'ERP n'a pas rendus (au-delà des plus récents), d'après leurs opérations.
    let connus: HashSet<u64> = e.bons_travail.iter().flatten().map(|b| b.id).collect();
    for bt in ordre_des_bons.iter().filter(|bt| !connus.contains(bt)) {
        let id = format!("bt-{bt}");
        let mut enfants: Vec<Tache> = operations[bt].iter().map(|o| tache_d_operation(o, &id, None)).collect();
        ranger_les_enfants(&mut enfants);
        let premier = enfants.iter().map(|t| t.debut.clone()).min();
        let dernier = enfants.iter().map(|t| t.fin.clone()).max();
        let (Some(debut), Some(fin)) = (premier, dernier) else { continue };
        let numero = operations[bt].iter().find_map(|o| o.bt_numero.clone());
        let titre = format!("Bon de travail {}", numero.clone().unwrap_or_else(|| bt.to_string()));
        let numero = numero.map(|n| format!(" {n}")).unwrap_or_default();
        let source = format!("le bon de travail{numero} (bon de travail n° {bt} de l'ERP), {}", du_au(&debut, &fin));
        let statut = statut_d_ensemble(&enfants);
        let tache = Tache {
            id,
            parent: None,
            titre,
            debut,
            fin,
            avancement: None,
            statut,
            prevu: prevu("bt", *bt),
            responsables: Vec::new(),
            source,
        };
        groupes.push((tache, enfants));
    }
    groupes.sort_by(|(a, _), (b, _)| (&a.debut, &a.titre).cmp(&(&b.debut, &b.titre)));
    let taches: Vec<Tache> = groupes.into_iter().flat_map(|(t, enfants)| std::iter::once(t).chain(enfants)).collect();

    // Les dépendances entre deux tâches du Gantt.
    let ids: HashSet<&str> = taches.iter().map(|t| t.id.as_str()).collect();
    let dependances = e
        .dependances
        .iter()
        .flatten()
        .filter_map(|d| {
            let (de, vers) = (id_de_tache(&d.de_sorte, &d.de_id)?, id_de_tache(&d.vers_sorte, &d.vers_id)?);
            let sorte = match d.sorte.as_str() {
                "finish_to_start" => "FS",
                "start_to_start" => "SS",
                "finish_to_finish" => "FF",
                "start_to_finish" => "SF",
                _ => return None,
            };
            let entre_deux = de != vers && ids.contains(de.as_str()) && ids.contains(vers.as_str());
            entre_deux.then(|| Dependance {
                de,
                vers,
                sorte: sorte.into(),
                decalage: d.decalage.clamp(-3650, 3650) as i32,
            })
        })
        .collect();
    (taches, dependances)
}

// ---- Le Kanban : les tableaux du suivi de l'ERP --------------------------------------------------------------------

/// Les tableaux du Kanban que le planning montre, dans cet ordre, avec leur titre. L'ERP en a un de plus,
/// `operations_sdp` (la vue fine du Dernier Planificateur), dont les cartes demandent ce qu'Eve ne lit pas.
const TABLEAUX: [(&str, &str); 7] = [
    ("projets", "Chantiers"),
    ("devis", "Soumissions"),
    ("bons_travail", "Bons de travail"),
    ("operations", "Opérations"),
    ("ventes", "Ventes"),
    ("achats", "Achats"),
    ("factures", "Factures"),
];

/// L'ordre des étapes du CRM de l'ERP (`modifier_opportunite` les nomme ainsi) : `suivi_colonnes` ne donne pas de
/// colonnes au tableau des ventes ; celles que ses comptes et ses cartes nomment prennent ce rang.
const ETAPES_DES_VENTES: [&str; 6] = ["prospection", "qualification", "proposition", "negociation", "gagne", "perdu"];

/// Les tableaux du Kanban : ceux dont l'ERP a dit les colonnes ou les comptes, ou dont Eve a lu des cartes.
fn kanban(e: &Entreprise) -> Vec<Tableau> {
    let lus: HashMap<&str, &TableauDuKanban> = e.kanban.iter().flatten().map(|t| (t.cle.as_str(), t)).collect();
    TABLEAUX
        .iter()
        .filter_map(|(cle, titre)| {
            let cartes = cartes_du_tableau(e, cle);
            let lu = lus.get(cle).copied();
            (lu.is_some() || !cartes.is_empty()).then(|| tableau(cle, titre, lu, cartes))
        })
        .collect()
}

/// Un tableau du Kanban : les colonnes de l'ERP dans son ordre, puis les statuts qu'il compte ou que portent les cartes
/// lues sans être l'une d'elles (un statut d'avant, une étape des ventes) ; dans chacune, combien de cartes l'ERP y
/// compte (au moins celles qu'Eve en a lues), et ces cartes. Un statut se reconnaît sans sa casse ni ses accents ; ceux
/// des chantiers et des opérations, ramenés d'abord à leur forme canonique, comme l'ERP le fait pour son Kanban
/// (« A_FAIRE » est « En attente »).
fn tableau(cle: &str, titre: &str, lu: Option<&TableauDuKanban>, cartes: Vec<(String, Carte)>) -> Tableau {
    let canonique = matches!(cle, "projets" | "operations");
    let canon = |s: &str| if canonique { statut_du_gantt(s) } else { s.to_string() };
    let cartes: Vec<(String, Carte)> = cartes.into_iter().map(|(s, c)| (canon(&s), c)).collect();
    let par_colonne: Vec<(String, u64)> =
        lu.into_iter().flat_map(|t| &t.par_colonne).map(|(s, n)| (canon(s), *n)).collect();
    let mut statuts: Vec<String> = lu.map(|t| t.statuts.clone()).unwrap_or_default();
    let mut autres: Vec<String> = Vec::new();
    let connu = |s: &str, l: &[String]| l.iter().any(|x| normaliser(x) == normaliser(s));
    let comptes = par_colonne.iter().map(|(s, _)| s.clone());
    for s in comptes.chain(cartes.iter().map(|(s, _)| s.clone())) {
        if !connu(&s, &statuts) && !connu(&s, &autres) {
            autres.push(s);
        }
    }
    if cle == "ventes" {
        let rang =
            |s: &String| ETAPES_DES_VENTES.iter().position(|x| *x == normaliser(s)).unwrap_or(ETAPES_DES_VENTES.len());
        autres.sort_by_key(rang);
    }
    statuts.extend(autres);
    let colonnes = statuts
        .into_iter()
        .map(|statut| {
            let n = normaliser(&statut);
            let lues: Vec<Carte> = cartes.iter().filter(|(s, _)| normaliser(s) == n).map(|(_, c)| c.clone()).collect();
            let compte: u64 = par_colonne.iter().filter(|(k, _)| normaliser(k) == n).map(|(_, v)| *v).sum();
            Colonne { titre: titre_de_statut(&statut), total: (compte as usize).max(lues.len()), cartes: lues, statut }
        })
        .collect();
    Tableau { cle: cle.into(), titre: titre.into(), colonnes, garde: lu.and_then(|t| t.garde.clone()) }
}

/// Les cartes qu'Eve a lues pour un tableau du Kanban, chacune avec son statut (vide : « (sans statut) », comme l'ERP
/// le compte). L'id d'une carte est celui de la tâche du Gantt qui montre la même chose.
fn cartes_du_tableau(e: &Entreprise, cle: &str) -> Vec<(String, Carte)> {
    let statut = |s: Option<&str>| {
        s.map(str::trim).filter(|s| !s.is_empty()).map_or_else(|| "(sans statut)".to_string(), str::to_string)
    };
    let dit = |s: &str| titre_de_statut(s).to_lowercase();
    let carte = |id: String, titre: String, detail: String, (montant, echeance): (Option<f64>, Option<String>)| Carte {
        id,
        titre,
        detail,
        montant,
        echeance,
        responsables: Vec::new(),
        source: String::new(),
    };
    let mut cartes: Vec<(String, Carte)> = Vec::new();
    match cle {
        "projets" => {
            for p in chantiers(e) {
                let id = cle_du_chantier(p);
                let detail = joint(&[p.numero.as_deref(), p.client.as_deref(), p.ville.as_deref()]);
                let s = statut(p.statut.as_deref());
                let mut c = carte(format!("projet-{id}"), p.nom.clone(), detail, (p.valeur, p.fin.clone()));
                let fin = p.fin.as_deref().map(|f| format!(", fin prévue {}", jour_dit(f))).unwrap_or_default();
                c.source = format!("{}{fin}, statut {}", chantier_dit(p), dit(&s));
                cartes.push((s, c));
            }
        }
        "devis" => {
            // Les soumissions en cours, et celles qui attendent une réponse (une seule fois chacune).
            let mut vus = HashSet::new();
            for d in e.soumissions.iter().flatten().chain(e.devis.iter().flatten()) {
                let id =
                    d.id.map(|i| format!("devis-{i}")).or_else(|| d.numero.clone()).unwrap_or_else(|| d.nom.clone());
                if !vus.insert(id.clone()) {
                    continue;
                }
                let s = statut(Some(&d.statut));
                let mut c =
                    carte(id, d.nom.clone(), joint(&[d.numero.as_deref(), d.client.as_deref()]), (d.valeur, None));
                let numero = d.numero.as_deref().map(|n| format!(" {n}")).unwrap_or_default();
                let de = d.client.as_deref().map(|c| format!(" pour {c}")).unwrap_or_default();
                let n = d.id.map(|i| format!(" (devis n° {i} de l'ERP)")).unwrap_or_default();
                let valeur = d.valeur.map(|v| format!(", {}", entreprise::dollars(v))).unwrap_or_default();
                c.source = format!("la soumission{numero} « {} »{de}{n}{valeur}, statut {}", d.nom, dit(&s));
                cartes.push((s, c));
            }
        }
        "bons_travail" => {
            for b in e.bons_travail.iter().flatten() {
                let s = statut(b.statut.as_deref());
                let titre =
                    b.nom.clone().or_else(|| b.numero.clone()).unwrap_or_else(|| format!("Bon de travail {}", b.id));
                let detail = joint(&[b.numero.as_deref(), b.projet.as_deref()]);
                let mut c = carte(format!("bt-{}", b.id), titre, detail, (b.montant, b.echeance.clone()));
                let echeance = b.echeance.as_deref().map(|e| format!(", à finir {}", jour_dit(e))).unwrap_or_default();
                c.source = format!("{}{echeance}, statut {}", bon_dit(b), dit(&s));
                cartes.push((s, c));
            }
        }
        "operations" => {
            let bons: HashMap<u64, &BonDeTravail> = e.bons_travail.iter().flatten().map(|b| (b.id, b)).collect();
            for o in e.operations.iter().flatten() {
                let bt = o.bt_id.and_then(|i| bons.get(&i).copied());
                // Le statut que l'ERP donne à son Kanban des opérations : une variante d'avant y prend sa colonne.
                let s = statut_du_gantt(o.statut.as_deref().unwrap_or(""));
                let detail = joint(&[o.bt_numero.as_deref(), bt.and_then(|b| b.projet.as_deref())]);
                let mut c = carte(format!("op-{}", o.id), o.nom.clone(), detail, (None, o.fin.clone()));
                c.responsables =
                    [o.responsable.as_deref(), sous_traitant(o)].into_iter().flatten().map(str::to_string).collect();
                c.source = format!("{}, statut {}", operation_dite(o, bt), dit(&s));
                cartes.push((s, c));
            }
        }
        "ventes" => {
            for o in e.opportunites.iter().flatten() {
                let s = statut(o.statut.as_deref());
                let chances = o.probabilite.map(|p| format!("{p} % de chances"));
                let detail = joint(&[chances.as_deref(), o.origine.as_deref()]);
                let mut c =
                    carte(format!("opportunite-{}", o.id), o.nom.clone(), detail, (o.montant, o.cloture.clone()));
                let montant = o.montant.map(|m| format!(", {}", entreprise::dollars(m))).unwrap_or_default();
                let chances = o.probabilite.map(|p| format!(", {p} % de chances")).unwrap_or_default();
                let cloture =
                    o.cloture.as_deref().map(|d| format!(", clôture prévue {}", jour_dit(d))).unwrap_or_default();
                c.source = format!(
                    "la vente « {} » (opportunité n° {} du CRM de l'ERP){montant}, à l'étape {}{chances}{cloture}",
                    o.nom,
                    o.id,
                    dit(&s)
                );
                cartes.push((s, c));
            }
        }
        "achats" => {
            for b in e.commandes.iter().flatten().filter(|c| c.sorte == "commande") {
                let id = b.id.map(|i| format!("bc-{i}")).or_else(|| b.numero.clone()).unwrap_or_default();
                let s = statut(b.statut.as_deref());
                let titre =
                    b.fournisseur.clone().or_else(|| b.numero.clone()).unwrap_or_else(|| "Bon de commande".into());
                let detail = joint(&[b.numero.as_deref(), b.projet.as_deref()]);
                let mut c = carte(id, titre, detail, (b.montant, b.date.clone()));
                let quoi = entre_parentheses(&[
                    b.fournisseur.as_deref().map(|f| format!("de {f}")).as_deref(),
                    b.id.map(|i| format!("n° {i} de l'ERP")).as_deref(),
                ]);
                let livraison =
                    b.date.as_deref().map(|d| format!(", livraison prévue {}", jour_dit(d))).unwrap_or_default();
                c.source = format!(
                    "le bon de commande {}{quoi}{livraison}, statut {}",
                    b.numero.as_deref().unwrap_or(""),
                    dit(&s)
                );
                cartes.push((s, c));
            }
        }
        "factures" => {
            for f in e.factures_ouvertes.iter().flatten() {
                let id = f.id.map(|i| format!("facture-{i}")).or_else(|| f.numero.clone()).unwrap_or_default();
                let s = statut(f.statut.as_deref());
                let sens = if f.sorte == "fournisseur" { "À payer" } else { "À recevoir" };
                let titre = f.tiers.clone().or_else(|| f.numero.clone()).unwrap_or_else(|| "Facture".into());
                let mut c =
                    carte(id, titre, joint(&[f.numero.as_deref(), Some(sens)]), (Some(f.solde), f.echeance.clone()));
                let echeance =
                    f.echeance.as_deref().map(|d| format!(", à échéance {}", jour_dit(d))).unwrap_or_default();
                c.source = format!("{}{echeance}, statut {}", facture_dite(f), dit(&s));
                cartes.push((s, c));
            }
        }
        _ => {}
    }
    cartes
}

// ---- L'entreprise entière -------------------------------------------------------------------------------------------

/// Le planning de l'entreprise d'après sa dernière lecture (`entreprise.json`) : ses événements du calendrier, les
/// tâches et les dépendances du Gantt, les tableaux du Kanban, et l'heure de la lecture (« 2026-10-28 14:02 »). Rien
/// d'une Eve qui n'est pas reliée à l'ERP. `aujourdhui` : une facture d'avant est échue ; un chantier commencé sans
/// date de fin dure au moins jusque-là.
pub fn de_l_entreprise(e: &Entreprise, aujourdhui: &str) -> Planning {
    if e.etat == entreprise::NON_CONNECTE {
        return Planning::default();
    }
    let (taches, dependances) = gantt(e, aujourdhui);
    Planning {
        evenements: evenements_de_l_entreprise(e, aujourdhui),
        taches,
        dependances,
        tableaux: kanban(e),
        lu: e.lu_a.as_deref().and_then(|a| a.get(..16)).map(|a| a.replace('T', " ")).unwrap_or_default(),
    }
}

#[cfg(test)]
mod essais {
    use super::*;
    use crate::entreprise::{DependanceDuGantt, Note};

    #[test]
    fn la_fenetre_va_du_mois_d_avant_au_deuxieme_mois_apres() {
        let f = |a: &str, b: &str| (a.to_string(), b.to_string());
        assert_eq!(fenetre("2026-10-28"), f("2026-09-01", "2026-12-31"));
        assert_eq!(fenetre("2027-01-15"), f("2026-12-01", "2027-03-31"), "d'une année à l'autre");
        assert_eq!(fenetre("2026-11-30"), f("2026-10-01", "2027-01-31"));
        assert_eq!(fenetre("2027-12-10"), f("2027-11-01", "2028-02-29"), "2028 est bissextile");
        assert_eq!(fenetre("hier"), f("", ""));
    }

    #[test]
    fn les_textes_de_l_erp_se_disent_en_francais() {
        assert_eq!(en_francais("Termine"), "Terminé");
        assert_eq!(en_francais("EN_ATTENTE"), "En attente");
        assert_eq!(en_francais("A_PLANIFIER"), "À planifier");
        assert_eq!(en_francais("A venir"), "À venir");
        assert_eq!(en_francais("Statut gere via le Bon de travail"), "Statut géré via le Bon de travail");
        assert_eq!(en_francais("Operations"), "Opérations");
        assert_eq!(en_francais("Terminé"), "Terminé", "déjà accentué : tel quel");
        assert_eq!(en_francais("En cours"), "En cours");
        assert_eq!(en_francais("Accepte (signé)."), "Accepté (signé).", "la ponctuation reste");
        assert_eq!(en_francais("Le devis a ete envoye"), "Le devis a ete envoyé", "« a » n'est « à » qu'en tête");
        // Les mots d'un statut ne prennent leurs accents que dans un statut.
        assert_eq!(en_francais("Bon de commande · Facture"), "Bon de commande · Facture");
        assert_eq!(
            (titre_de_statut("COMMANDE"), titre_de_statut("PRET_A_FACTURER")),
            ("Commandé".into(), "Prêt à facturer".into())
        );
    }

    #[test]
    fn les_jours_et_les_statuts_se_disent() {
        assert_eq!(jour_dit("2026-10-27"), "le mardi 27 octobre");
        assert_eq!(jour_dit("2026-11-01"), "le dimanche 1er novembre");
        assert_eq!(du_au("2026-10-27", "2026-10-29"), "du mardi 27 octobre au jeudi 29 octobre");
        assert_eq!(du_au("2026-10-27", "2026-10-27"), "le mardi 27 octobre");
        for (statut, titre) in [
            ("EN_COURS", "En cours"),
            ("Termine", "Terminé"),
            ("PARTIELLEMENT_PAYEE", "Partiellement payée"),
            ("A_DEGAGER", "À dégager"),
            ("Recu", "Reçu"),
            ("commande", "Commandé"),
            ("GAGNE", "Gagné"),
            ("Négociation", "Négociation"),
            ("(sans statut)", "Sans statut"),
            ("", "Sans statut"),
        ] {
            assert_eq!(titre_de_statut(statut), titre, "{statut}");
        }
        for (statut, canon) in [
            ("EN_COURS", "En cours"),
            ("A_FAIRE", "En attente"),
            ("", "En attente"),
            ("(sans statut)", "En attente"),
            ("BROUILLON", "En attente"),
            ("TERMINÉE", "Termine"),
            ("Annulée", "Annule"),
            ("EN_PAUSE", "Suspendu"),
            ("Inconnu de l'ERP ", "Inconnu de l'ERP"),
        ] {
            assert_eq!(statut_du_gantt(statut), canon, "{statut}");
        }
        assert_eq!(heure_du_calendrier("8:30").as_deref(), Some("08:30"));
        assert_eq!(heure_du_calendrier("08:30:00").as_deref(), Some("08:30"));
        assert_eq!((heure_du_calendrier("25:00"), heure_du_calendrier("midi")), (None, None));
        assert_eq!((heures(12.0), heures(7.5), heures(10.0)), ("12".to_string(), "7,5".to_string(), "10".to_string()));
        assert_eq!(entre_parentheses(&[None, Some(" "), Some("a"), Some("b")]), " (a, b)");
        assert_eq!(entre_parentheses(&[None]), "");
    }

    /// Une entreprise inventée, pour les cas qu'une lecture du faux serveur ne montre pas : un chantier sans dates, lu
    /// deux fois ; une opération qui finit avant de commencer, et qui a plus d'heures faites que prévues ; un bon de
    /// travail qui n'a que son échéance ; des dépendances que le Gantt n'a pas ; une note sans date lisible.
    fn entreprise_inventee() -> Entreprise {
        let projet = |id: u64, nom: &str, statut: &str, dates: (Option<&str>, Option<&str>)| Projet {
            id: Some(id),
            nom: nom.into(),
            statut: Some(statut.into()),
            debut: dates.0.map(Into::into),
            fin: dates.1.map(Into::into),
            ..Projet::default()
        };
        let lien = |de: (&str, &str), vers: (&str, &str), sorte: &str, decalage: i64| DependanceDuGantt {
            de_sorte: de.0.into(),
            de_id: de.1.into(),
            vers_sorte: vers.0.into(),
            vers_id: vers.1.into(),
            sorte: sorte.into(),
            decalage,
        };
        Entreprise {
            etat: entreprise::A_JOUR.into(),
            lu_a: Some("2026-10-28T14:02:11-04:00".into()),
            projets: Some(vec![
                projet(1, "Chalet, lac de l'Essai", "En cours", (None, None)),
                projet(2, "Duplex, rue des Exemples", "EN_COURS", (Some("2026-10-01"), Some("2026-12-15"))),
            ]),
            projets_en_attente: Some(vec![projet(2, "Duplex, rue des Exemples", "En attente", (None, None))]),
            bons_travail: Some(vec![
                BonDeTravail {
                    id: 7,
                    numero: Some("BT-7".into()),
                    nom: Some("Toiture".into()),
                    statut: Some("EN_COURS".into()),
                    projet_id: Some(1),
                    projet: Some("Chalet, lac de l'Essai".into()),
                    ..BonDeTravail::default()
                },
                BonDeTravail {
                    id: 8,
                    numero: Some("BT-8".into()),
                    statut: Some("BROUILLON".into()),
                    projet_id: Some(2),
                    echeance: Some("2026-11-20".into()),
                    ..BonDeTravail::default()
                },
            ]),
            operations: Some(vec![Operation {
                id: 70,
                bt_id: Some(7),
                nom: "Charpente".into(),
                statut: Some("En cours".into()),
                debut: Some("2026-10-20".into()),
                fin: Some("2026-10-18".into()),
                heures_prevues: Some(10.0),
                heures_reelles: Some(15.0),
                ..Operation::default()
            }]),
            dependances: Some(vec![
                lien(("op", "70"), ("bt", "8"), "start_to_start", 2),
                lien(("op", "70"), ("op", "70"), "finish_to_start", 0),
                lien(("project", "1"), ("project", "2"), "lien_inconnu", 0),
                lien(("project", "1"), ("project", "99"), "finish_to_start", 0),
            ]),
            agenda: Some(vec![
                Note { quand: "2026-10-30".into(), texte: "Inspection".into() },
                Note { quand: "pas une date".into(), texte: "Perdue".into() },
            ]),
            ..Entreprise::default()
        }
    }

    #[test]
    fn le_gantt_des_chantiers_et_de_leurs_operations() {
        let p = de_l_entreprise(&entreprise_inventee(), "2026-10-28");
        let vu: Vec<(&str, Option<&str>, &str, &str)> =
            p.taches.iter().map(|t| (t.id.as_str(), t.parent.as_deref(), t.debut.as_str(), t.fin.as_str())).collect();
        assert_eq!(
            vu,
            [
                // Lu deux fois (en cours, puis à venir), il ne se montre qu'une ; son bon de travail, à son échéance.
                ("projet-2", None, "2026-10-01", "2026-12-15"),
                ("bt-8", Some("projet-2"), "2026-11-20", "2026-11-20"),
                // Sans dates, un chantier prend celles de son opération ; en cours, il dure jusqu'à aujourd'hui.
                ("projet-1", None, "2026-10-20", "2026-10-28"),
                // Une fin d'avant le début : un seul jour.
                ("op-70", Some("projet-1"), "2026-10-20", "2026-10-20"),
            ]
        );
        assert_eq!((p.taches[0].statut.as_str(), p.taches[1].statut.as_str()), ("En cours", "En attente"));
        assert_eq!(p.taches[3].avancement, Some(1.0), "plus d'heures faites que prévues : au plus 1");
        assert!(p.taches[2].source.contains("sans date de fin dans l'ERP"), "{}", p.taches[2].source);
        // Les dépendances entre deux tâches du Gantt, d'une sorte connue ; ni d'une tâche à elle-même.
        let deps: Vec<(&str, &str, &str, i32)> =
            p.dependances.iter().map(|d| (d.de.as_str(), d.vers.as_str(), d.sorte.as_str(), d.decalage)).collect();
        assert_eq!(deps, [("op-70", "bt-8", "SS", 2)]);
        assert_eq!(p.lu, "2026-10-28 14:02");
        // Le calendrier : une note sans date lisible ne s'y place pas ; un bon de travail, à son échéance.
        assert!(p.evenements.iter().any(|e| e.titre == "Inspection" && e.sorte == Sorte::Note));
        assert!(!p.evenements.iter().any(|e| e.titre == "Perdue"));
        let bt = p.evenements.iter().find(|e| e.titre == "BT-8").unwrap();
        assert_eq!(
            (bt.debut.as_str(), bt.fin.as_deref(), bt.sorte, bt.cote),
            ("2026-11-20", None, Sorte::Operation, Cote::Entreprise)
        );
        // Le Kanban sans ce que l'ERP en dit : les colonnes que portent les cartes lues, leurs variantes réunies.
        let chantiers = p.tableaux.iter().find(|t| t.cle == "projets").unwrap();
        let colonnes: Vec<(&str, usize)> = chantiers.colonnes.iter().map(|c| (c.statut.as_str(), c.total)).collect();
        assert_eq!(colonnes, [("En cours", 2)]);
        assert!(p.tableaux.iter().all(|t| t.garde.is_none()));
        // Pas reliée à l'ERP : rien de l'entreprise, même d'une lecture d'avant.
        let pas_reliee = Entreprise { etat: entreprise::NON_CONNECTE.into(), ..entreprise_inventee() };
        assert_eq!(de_l_entreprise(&pas_reliee, "2026-10-28"), Planning::default());
    }

    /// Un `entreprise.json` d'avant qu'Eve garde le numéro des projets dans l'ERP : ses chantiers se montrent quand
    /// même, sous leur numéro de document.
    #[test]
    fn un_chantier_sans_numero_de_l_erp_se_montre_quand_meme() {
        let chantier = Projet {
            nom: "Résidence".into(),
            numero: Some("PROJ-2026-00031".into()),
            statut: Some("En cours".into()),
            debut: Some("2026-09-18".into()),
            fin: Some("2026-12-17".into()),
            ..Projet::default()
        };
        let e = Entreprise { etat: entreprise::A_JOUR.into(), projets: Some(vec![chantier]), ..Entreprise::default() };
        let p = de_l_entreprise(&e, "2026-10-28");
        assert_eq!(p.taches.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["projet-PROJ-2026-00031"]);
        assert_eq!(p.tableaux[0].colonnes[0].cartes[0].id, "projet-PROJ-2026-00031");
        assert_eq!(p.taches[0].prevu, None);
    }
}
