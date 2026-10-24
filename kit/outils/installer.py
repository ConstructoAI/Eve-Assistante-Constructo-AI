# -*- coding: utf-8 -*-
"""L'installateur d'Eve : une entrevue bâtit, question par question, le profil complet d'un spécialiste, puis Eve
devient ce spécialiste. On le relance pour en ajouter un autre.

    Installer.bat                       le double-clic (programme\\python\\python.exe programme\\outils\\installer.py)
    installer.py --essai reponses.json  un essai sans clavier : les réponses viennent du fichier, rien ne se lance

Ce qu'il pose sur le poste, pour tous les spécialistes :
    %LOCALAPPDATA%\\Eve\\programme\\            le programme, recopié du kit (une Eve qui tourne garde l'ancien)
    %LOCALAPPDATA%\\Eve\\commun\\               la voix et l'oreille, téléchargées une fois (environ 410 Mo)
    %LOCALAPPDATA%\\Eve\\specialistes.json      la liste des spécialistes posés
    %LOCALAPPDATA%\\Eve\\Installer.bat          de quoi en ajouter un autre sans le kit
et pour chacun :
    %LOCALAPPDATA%\\Eve\\specialistes\\<id>\\    ses réglages, son journal, sa mémoire, son historique
    <son dossier de travail>\\                  PROFIL.md, CLAUDE.md, EVE_CONSIGNE.md, entrevue.json, scripts\\, Eve.bat

Il faut Claude Code sur le poste, connecté au compte Claude de la personne : c'est le cerveau d'Eve, et l'entrevue
comme l'écriture du profil passent par lui.
"""
import argparse
import datetime
import hashlib
import json
import os
import re
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import time
import unicodedata

ICI = os.path.dirname(os.path.abspath(__file__))
KIT = os.path.dirname(ICI)
GABARITS = os.path.join(ICI, "gabarits")

EVE = os.path.join(os.environ.get("LOCALAPPDATA") or os.path.join(os.path.expanduser("~"), "AppData", "Local"), "Eve")
PROGRAMME = os.path.join(EVE, "programme")
COMMUN = os.path.join(EVE, "commun")
SPECIALISTES = os.path.join(EVE, "specialistes")
REGISTRE = os.path.join(EVE, "specialistes.json")

# L'entrevue doit répondre en quelques secondes (mesuré : 4,7 s le tour) ; le profil, lui, s'écrit avec le plus fort.
ENTREVUE = ("claude-sonnet-5-5", "low")
REDACTION = ("claude-opus-5-5", "high")
# Les choix de la rubrique « Cerveau » de la fenêtre (src\reglages.rs, MODELES et EFFORTS) : Sonnet ou Opus, l'effort
# moyen ou max ; elle les change ensuite.
CERVEAUX = [
    ("Sonnet 5.5 : rapide, recommandé pour la voix", "claude-sonnet-5-5", "medium"),
    ("Opus 5.5 : plus fort, plus lent", "claude-opus-5-5", "medium"),
    ("Opus 5.5, réflexion maximale : le plus fort ; une réponse peut prendre plusieurs minutes", "claude-opus-5-5", "max"),
]
# Les touches que connaît le programme, sous les noms qu'affiche sa fenêtre (src\reglages.rs, Reglages::touche).
TOUCHES = [
    ("Ctrl droit", "ctrl_droit"),
    ("F8", "f8"),
    ("F9", "f9"),
    ("Pause", "pause"),
    ("Arrêt défil", "arret_defil"),
    ("Alt droit", "alt_droit"),
    ("Ctrl gauche", "ctrl_gauche"),
]
# Les deux allures de la fenêtre (le réglage « theme », src\reglages.rs) : le choix revient à la personne, et se change
# ensuite dans les réglages de la fenêtre. La première est celle d'office du programme : rien ne s'écrit pour elle.
THEMES = [
    ("Le centre de commande : le réseau neuronal, ses panneaux et sa musique", "le centre de commande", None),
    ("Le thème clair Constructo AI : le tableau de bord de l'ERP, sans musique", "le thème clair Constructo AI",
     "constructo"),
]
PORT_DEPART = 47821
QUESTIONS_MAX = 24
# En dessous, le profil est trop mince : une seconde passe le complète (le plombier de référence en fait 47 000).
PROFIL_MIN = 15000
REDACTION_MAX = 45 * 60
PROGRAMME_FICHIERS = ("eve.exe", "msvcp140.dll", "vcruntime140.dll", "vcruntime140_1.dll")
# Le serveur de Whisper pour carte NVIDIA est celui de whisper.cpp pour CUDA 12.4 : sous Windows, il veut un pilote
# NVIDIA 551.61 ou plus récent.
PILOTE_MIN = (551, 61)
FICHIERS_DU_DOSSIER = ("CLAUDE.md", "PROFIL.md", "EVE_CONSIGNE.md", "Eve.bat", "entrevue.json")
PREMIERE_QUESTION = (
    "Quel spécialiste veux-tu ? (par exemple : spécialiste des voyages du Québec vers le Sud, garagiste, comptable "
    "pour travailleurs autonomes, entraîneur de course à pied)"
)
FIN = {"assez", "fin", "stop", "c'est assez", "ça suffit", "ca suffit", "c'est bon", "terminé", "termine"}

SCHEMA_TOUR = {
    "type": "object",
    "properties": {
        "acquis": {"type": "string"},
        "question": {"type": "string"},
        "couverture": {"type": "integer", "minimum": 0, "maximum": 100},
        "termine": {"type": "boolean"},
        "nom": {"type": "string"},
    },
    "required": ["acquis", "question", "couverture", "termine", "nom"],
    "additionalProperties": False,
}


class Echec(Exception):
    """Une raison de s'arrêter, dite à la personne telle quelle."""


# ---------------------------------------------------------------------------------------------------------------------
# Les réponses : le clavier, ou le fichier d'un essai
# ---------------------------------------------------------------------------------------------------------------------
class Reponses:
    """Ce que la personne tape ; en essai, ce que porte le fichier : « entrevue » dans l'ordre (puis « assez »), et
    « communes » par question."""

    def __init__(self, essai=None):
        self.essai = None
        if essai:
            with open(essai, encoding="utf-8") as f:
                self.essai = json.load(f)
            self.reste = list(self.essai.get("entrevue", []))

    def entrevue(self, invite):
        if self.essai is None:
            return self._clavier(invite)
        r = str(self.reste.pop(0)) if self.reste else "assez"
        print(invite + r, flush=True)
        return r

    def commune(self, cle, invite):
        if self.essai is None:
            return self._clavier(invite)
        r = str(self.essai.get("communes", {}).get(cle, ""))
        print(invite + r, flush=True)
        return r

    @staticmethod
    def _clavier(invite):
        try:
            return input(invite).strip()
        except EOFError:
            raise Echec("Plus rien à lire au clavier : l'installation s'arrête.") from None


def oui_non(reponses, cle, question, defaut):
    choix = "O/n" if defaut else "o/N"
    while True:
        r = reponses.commune(cle, f"\n  {question} ({choix})\n  > ").strip().lower()
        if not r:
            return defaut
        if r in ("o", "oui", "y", "yes"):
            return True
        if r in ("n", "non", "no"):
            return False
        if reponses.essai is not None:
            raise Echec(f"Essai : « {r} » n'est ni oui ni non ({cle}).")
        print("  Réponds par o (oui) ou n (non).")


def choisir(reponses, cle, question, options, defaut):
    """Une option par son numéro ; rend son rang (à partir de 0)."""
    print(f"\n  {question}")
    for i, texte in enumerate(options, 1):
        print(f"    {i}) {texte}{'   (par défaut)' if i - 1 == defaut else ''}")
    while True:
        r = reponses.commune(cle, "  > ").strip()
        if not r:
            return defaut
        if r.isdigit() and 1 <= int(r) <= len(options):
            return int(r) - 1
        if reponses.essai is not None:
            raise Echec(f"Essai : « {r} » n'est pas un choix ({cle}).")
        print(f"  Tape un nombre de 1 à {len(options)}, ou Entrée pour le choix par défaut.")


# ---------------------------------------------------------------------------------------------------------------------
# Claude Code
# ---------------------------------------------------------------------------------------------------------------------
def trouver_claude():
    """Claude Code : les mêmes endroits, dans le même ordre, que ceux où Eve le cherche (cerveau.rs)."""
    candidats = []
    if os.environ.get("APPDATA"):
        candidats.append(os.path.join(os.environ["APPDATA"], "npm", "node_modules", "@anthropic-ai", "claude-code",
                                      "bin", "claude.exe"))
    candidats.append(os.path.join(os.path.expanduser("~"), ".local", "bin", "claude.exe"))
    candidats += [os.path.join(d, "claude.exe") for d in os.environ.get("PATH", "").split(os.pathsep) if d]
    return next((c for c in candidats if os.path.isfile(c)), None)


def env_claude():
    # Ni les connecteurs de claude.ai ni les serveurs MCP : l'entrevue et l'écriture du profil n'en ont pas besoin, et
    # chacun coûte du temps à chaque appel.
    return dict(os.environ, ENABLE_CLAUDEAI_MCP_SERVERS="false")


def appel(claude, modele, systeme, message, schema=None, temps_max=180):
    """Un tour de Claude Code sans outil, sans serveur MCP, sans réglage du poste ; rend son objet de résultat."""
    args = [claude, "-p", "--model", modele[0], "--effort", modele[1], "--tools", "", "--strict-mcp-config",
            "--setting-sources", "", "--no-session-persistence", "--output-format", "json", "--system-prompt", systeme]
    if schema:
        args += ["--json-schema", json.dumps(schema, ensure_ascii=False)]
    with tempfile.TemporaryDirectory(prefix="eve-") as vide:
        try:
            r = subprocess.run(args, input=message, capture_output=True, text=True, encoding="utf-8", errors="replace",
                               cwd=vide, env=env_claude(), timeout=temps_max)
        except subprocess.TimeoutExpired:
            raise Echec(f"Claude Code n'a pas répondu en {temps_max} s.") from None
    try:
        o = json.loads(r.stdout)
    except ValueError:
        detail = [l for l in (r.stderr or r.stdout or "").strip().splitlines() if l.strip()]
        raise Echec("Claude Code n'a pas répondu comme prévu : " + (detail[-1] if detail else f"code {r.returncode}")) from None
    if o.get("is_error"):
        raise Echec("Claude Code : " + str(o.get("result") or o.get("subtype") or "erreur"))
    return o


def objet(o):
    """L'objet structuré du tour ; un Claude Code plus ancien le rend en texte."""
    s = o.get("structured_output")
    if isinstance(s, dict):
        return s
    texte = re.sub(r"^```(?:json)?\s*|\s*```$", "", str(o.get("result") or "").strip())
    try:
        s = json.loads(texte)
    except ValueError:
        s = None
    if not isinstance(s, dict):
        raise Echec("Claude Code n'a pas rendu l'objet attendu : il faut une version récente (claude update).")
    return s


def verifier_claude():
    claude = trouver_claude()
    if not claude:
        raise Echec(
            "Claude Code est introuvable sur ce poste : c'est le cerveau d'Eve.\n"
            "  Pour l'installer : ouvre PowerShell et tape   irm https://claude.ai/install.ps1 | iex\n"
            "  puis tape   claude   pour te connecter à ton compte Claude, et relance Installer.bat.\n"
            "  Le guide : https://docs.claude.com/fr/docs/claude-code/setup"
        )
    print("  Claude Code : " + claude)
    print("  Je vérifie qu'il répond…", flush=True)
    try:
        appel(claude, ENTREVUE, "Tu réponds en un seul mot.", "Réponds seulement : prêt", temps_max=120)
    except Echec as e:
        raise Echec(
            f"{e}\n"
            "  S'il faut se connecter : ouvre une invite de commandes, tape   claude   et connecte-toi à ton compte "
            "Claude, puis relance Installer.bat."
        ) from None
    print("  Il répond.")
    return claude


# ---------------------------------------------------------------------------------------------------------------------
# L'entrevue
# ---------------------------------------------------------------------------------------------------------------------
def barre(pourcent):
    p = max(0, min(100, int(pourcent)))
    n = p // 5
    return f"Profil : [{'#' * n}{'.' * (20 - n)}] {p} %"


def message_d_entrevue(echanges):
    lignes = ["L'entrevue jusqu'ici :"]
    for i, e in enumerate(echanges, 1):
        lignes += [f"Q{i} : {e['question']}", f"R{i} : {e['reponse']}"]
    lignes += ["", "Rends le JSON du prochain tour."]
    return "\n".join(lignes)


def entrevue(claude, reponses):
    systeme = gabarit("entrevue.md")
    print(
        "\n  L'ENTREVUE. Je te pose des questions, une à la fois, et le profil se bâtit avec tes réponses.\n"
        "  Réponds comme tu veux ; « je ne sais pas » est une bonne réponse. Tape « assez » quand tu veux arrêter."
    )
    echanges, nom, couverture, question = [], "", 0, PREMIERE_QUESTION
    while len(echanges) < QUESTIONS_MAX:
        r = " ".join(reponses.entrevue(f"\n  {question}\n  > ").split())
        if r.lower().strip(" .!") in FIN:
            if echanges:
                break
            print("  Il me faut au moins le métier pour commencer.")
            continue
        echanges.append({"question": question, "reponse": r or "(pas de réponse)"})
        print("  …", flush=True)
        tour = objet(appel(claude, ENTREVUE, systeme, message_d_entrevue(echanges), SCHEMA_TOUR, temps_max=150))
        acquis = str(tour.get("acquis", "")).strip()
        nom = str(tour.get("nom", "")).strip() or nom
        couverture = int(tour.get("couverture", 0) or 0)
        if acquis:
            print("  + " + acquis)
        print("  " + barre(couverture))
        if tour.get("termine"):
            break
        question = str(tour.get("question", "")).strip() or "Y a-t-il autre chose que ce spécialiste doit savoir ?"
    if couverture < 80:
        print(f"\n  Le profil est couvert à {couverture} % : l'écriture complétera le reste au mieux, comme le ferait un "
              "expert du métier.")
    return echanges, nom


# ---------------------------------------------------------------------------------------------------------------------
# Les questions communes
# ---------------------------------------------------------------------------------------------------------------------
def registre():
    try:
        with open(REGISTRE, encoding="utf-8") as f:
            r = json.load(f)
        return r if isinstance(r, list) else []
    except (OSError, ValueError):
        return []


def identifiant(nom, pris):
    base = unicodedata.normalize("NFKD", nom).encode("ascii", "ignore").decode("ascii").lower()
    base = re.sub(r"[^a-z0-9]+", "-", base).strip("-")[:40] or "specialiste"
    ident, n = base, 2
    while ident in pris or os.path.exists(os.path.join(SPECIALISTES, ident)):
        ident, n = f"{base}-{n}", n + 1
    return ident


def nom_de_fichier(nom):
    propre = re.sub(r'[<>:"/\\|?*\x00-\x1f]', " ", nom)
    propre = " ".join(propre.split()).rstrip(" .")
    return propre or "Specialiste"


def demander_nom(reponses, suggere, pris):
    suggere = suggere or "Spécialiste"
    while True:
        r = " ".join(reponses.commune("nom", f"\n  Son nom dans la fenêtre d'Eve ? [{suggere}]\n  > ").split()) or suggere
        probleme = None
        if len(r) > 40 or any(unicodedata.category(c) == "Cc" for c in r):
            probleme = "Un nom de 40 caractères au plus, s'il te plaît."
        elif r.lower() == "adjointe":
            probleme = "« Adjointe » est réservé à l'Eve d'Entrepreneur Général : choisis un autre nom."
        elif r.lower() in pris:
            probleme = "Un autre spécialiste porte déjà ce nom sur ce poste."
        if not probleme:
            return r
        if reponses.essai is not None:
            raise Echec(f"Essai : {probleme}")
        print("  " + probleme)


def demander_dossier(reponses, nom):
    defaut = os.path.join(os.path.expanduser("~"), "Eve", nom_de_fichier(nom))
    while True:
        r = reponses.commune("dossier", f"\n  Son dossier de travail (ses notes, ses documents) ? [{defaut}]\n  > ")
        r = os.path.abspath(os.path.expandvars(r.strip().strip('"'))) if r.strip() else defaut
        deja = [f for f in FICHIERS_DU_DOSSIER if os.path.exists(os.path.join(r, f))]
        if deja:
            probleme = f"Ce dossier a déjà {', '.join(deja)} : un autre spécialiste y vit. Choisis-en un autre."
        elif os.path.isfile(r):
            probleme = "C'est un fichier, pas un dossier."
        else:
            probleme = None
        if not probleme:
            if os.path.isdir(r) and os.listdir(r) and not oui_non(
                    reponses, "dossier_plein", "Ce dossier n'est pas vide : y mettre quand même le spécialiste ?", False):
                continue
            return r
        if reponses.essai is not None:
            raise Echec(f"Essai : {probleme}")
        print("  " + probleme)


def port_libre(pris):
    for p in range(PORT_DEPART, PORT_DEPART + 400):
        if p in pris:
            continue
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
            try:
                s.bind(("127.0.0.1", p))
            except OSError:
                continue
        return p
    raise Echec("Aucun port libre pour la fenêtre d'Eve.")


def questions_communes(reponses, nom_suggere):
    deja = registre()
    print("\n  QUELQUES RÉGLAGES, et c'est fini.")
    prenom = " ".join(reponses.commune("prenom", "\n  Ton prénom ? (Eve s'en sert pour te parler)\n  > ").split())[:40]
    nom = demander_nom(reponses, nom_suggere, {s.get("nom", "").lower() for s in deja})
    dossier = demander_dossier(reponses, nom)
    cerveau = CERVEAUX[choisir(reponses, "cerveau", "Le cerveau d'Eve :", [c[0] for c in CERVEAUX], 0)]
    prises = {s.get("touche") for s in deja}
    libre = next((i for i, (_, t) in enumerate(TOUCHES) if t not in prises), 0)
    touche = TOUCHES[choisir(reponses, "touche", "La touche à tenir pendant que tu parles :",
                             [t[0] + ("   (déjà prise par un autre spécialiste)" if t[1] in prises else "")
                              for t in TOUCHES], libre)]
    if any(s.get("eveil") for s in deja):
        print("\n  Attention : un autre spécialiste répond déjà à « Eve, … ». Si les deux tournent en même temps, les "
              "deux répondront.")
    eveil = oui_non(reponses, "eveil", "La réveiller à la voix, en commençant tes phrases par « Eve, … » ?", True)
    carte = demander_carte(reponses)
    # Ce qu'Eve peut faire : d'office, tout (le 24 octobre 2026 : « lui donner la liberté totale ») ;
    # Constructo AI, la raison d'être du programme ; et les accords à la voix, gardés tant qu'on ne les retire pas.
    liberte = oui_non(reponses, "liberte", "Lui laisser la liberté totale ? Tous les outils de Claude Code (les "
                      "commandes, plusieurs agents en parallèle), Chrome, les connecteurs de claude.ai et les serveurs "
                      "MCP de ce poste. Non : ses fichiers et le Web seulement.", True)
    constructo = oui_non(reponses, "constructo", "La relier à Constructo AI, l'ERP de ton entreprise ? Au premier "
                         "lancement, Eve ouvre sa page de connexion dans ton navigateur (le courriel et le mot de passe "
                         "de ton entreprise).", True)
    accords = oui_non(reponses, "accords", "Avant de supprimer, ou d'écrire dans Constructo AI (créer, modifier, payer, "
                      "envoyer…), Eve te demande-t-elle « je confirme » ?", True)
    theme = THEMES[choisir(reponses, "theme", "Son allure (elle se change ensuite dans ses réglages) :",
                           [t[0] for t in THEMES], 0)]
    raccourci = oui_non(reponses, "raccourci", "Un raccourci sur le bureau ?", True)
    return {
        "prenom": prenom, "nom": nom, "dossier": dossier, "modele": cerveau[1],
        "effort": cerveau[2], "cerveau": cerveau[0], "touche": touche[1], "touche_nom": touche[0], "eveil": eveil,
        "raccourci": raccourci, "carte": carte, "liberte": liberte, "constructo": constructo, "accords": accords,
        "theme": theme[2], "theme_nom": theme[1],
        "id": identifiant(nom, {s.get("id") for s in deja}),
        "port": port_libre({s.get("port") for s in deja}),
    }


def carte_nvidia():
    """La carte NVIDIA du poste et la version de son pilote, par nvidia-smi (posé avec le pilote) : (nom, pilote), ou
    None sans carte NVIDIA ou sans pilote."""
    try:
        r = subprocess.run(["nvidia-smi", "--query-gpu=name,driver_version", "--format=csv,noheader"],
                           capture_output=True, text=True, timeout=20)
    except (OSError, subprocess.SubprocessError):
        return None
    lignes = (r.stdout or "").strip().splitlines()
    if r.returncode != 0 or not lignes or "," not in lignes[0]:
        return None
    nom, _, pilote = lignes[0].rpartition(",")
    return nom.strip(), pilote.strip()


def pilote_suffit(pilote):
    """« 582.28 » suffit à CUDA 12.4 ; « 537.13 », non."""
    try:
        return tuple(int(x) for x in pilote.split(".")[:2]) >= PILOTE_MIN
    except ValueError:
        return False


def demander_carte(reponses):
    """L'oreille sur la carte graphique : une carte NVIDIA transcrit bien plus vite que le processeur (mesuré sur une
    GTX 1050 Ti : 12,4 s de parole en 0,9 s ; sur le processeur d'un poste de 2012, 4,4 s de parole en 7,6 s). Déjà
    téléchargée pour un autre spécialiste, elle sert d'office."""
    if os.path.isfile(os.path.join(COMMUN, "whisper-gpu", "whisper-server.exe")):
        return True
    carte = carte_nvidia()
    if carte is None:
        return False
    nom, pilote = carte
    if not pilote_suffit(pilote):
        print(f"\n  Ta carte {nom} ferait entendre Eve bien plus vite, mais son pilote ({pilote}) est trop ancien : il "
              "faut la version 551.61 ou plus. Mets-le à jour (nvidia.com), puis relance l'installation pour l'ajouter.")
        return False
    return oui_non(reponses, "carte", f"Ta carte {nom} peut faire entendre Eve bien plus vite. Télécharger son "
                   "oreille pour la carte (1,3 Go, une seule fois) ?", True)


# ---------------------------------------------------------------------------------------------------------------------
# Les fichiers
# ---------------------------------------------------------------------------------------------------------------------
def gabarit(nom, **valeurs):
    with open(os.path.join(GABARITS, nom), encoding="utf-8") as f:
        texte = f.read()
    for cle, valeur in valeurs.items():
        texte = texte.replace("{{" + cle + "}}", str(valeur))
    return texte


def ecrire(chemin, texte):
    with open(chemin, "w", encoding="utf-8", newline="\n") as f:
        f.write(texte)


def ecrire_bat(chemin, texte):
    """Un .bat : ASCII et CRLF, sans quoi cmd.exe le lit de travers."""
    texte.encode("ascii")
    with open(chemin, "w", encoding="ascii", newline="\r\n") as f:
        f.write(texte)


def empreinte(chemin):
    h = hashlib.sha256()
    with open(chemin, "rb") as f:
        for bloc in iter(lambda: f.read(1 << 20), b""):
            h.update(bloc)
    return h.hexdigest()


def nombre(n):
    """76685 -> « 76 685 »."""
    return f"{n:,}".replace(",", " ")


def montant(x):
    """2.346 -> « 2,35 »."""
    return f"{x:.2f}".replace(".", ",")


def aujourdhui():
    mois = ["janvier", "février", "mars", "avril", "mai", "juin", "juillet", "août", "septembre", "octobre", "novembre",
            "décembre"]
    d = datetime.date.today()
    return f"{d.day} {mois[d.month - 1]} {d.year}"


# ---------------------------------------------------------------------------------------------------------------------
# Sur le poste
# ---------------------------------------------------------------------------------------------------------------------
def meme_dossier(a, b):
    try:
        return os.path.samefile(a, b)
    except OSError:
        return False


def poser_programme():
    """Le programme dans %LOCALAPPDATA%\\Eve\\programme. Une Eve qui tourne bloque l'écrasement de son programme, pas
    son renommage : l'ancien devient *.remplace.*, que le lanceur retire dès qu'il ne tourne plus."""
    os.makedirs(PROGRAMME, exist_ok=True)
    if meme_dossier(KIT, PROGRAMME):
        return
    for nom in PROGRAMME_FICHIERS:
        source, cible = os.path.join(KIT, nom), os.path.join(PROGRAMME, nom)
        if not os.path.isfile(source):
            raise Echec(f"Le kit est incomplet : programme\\{nom} manque.")
        if os.path.isfile(cible) and empreinte(cible) == empreinte(source):
            continue
        try:
            shutil.copy2(source, cible)
        except PermissionError:
            racine, ext = os.path.splitext(cible)
            vieux = f"{racine}.remplace{ext}"
            try:
                os.remove(vieux)
            except OSError:
                pass
            os.replace(cible, vieux)
            shutil.copy2(source, cible)
    for d in ("python", "outils"):
        shutil.copytree(os.path.join(KIT, d), os.path.join(PROGRAMME, d), dirs_exist_ok=True)
    installeur = os.path.join(os.path.dirname(KIT), "Installer.bat")
    if os.path.isfile(installeur):
        shutil.copy2(installeur, os.path.join(EVE, "Installer.bat"))


def lancer_telechargement(carte):
    """La voix et l'oreille, dans commun\\, pendant que le profil s'écrit : rien à refaire si elles y sont déjà ;
    `carte` : l'oreille de la carte NVIDIA aussi."""
    os.makedirs(COMMUN, exist_ok=True)
    journal = open(os.path.join(EVE, "telechargement.log"), "w", encoding="utf-8")
    p = subprocess.Popen(
        [sys.executable, "-X", "utf8", "-s", os.path.join(PROGRAMME, "outils", "ressources_eve.py"), "--installer",
         "--dossier", COMMUN] + (["--carte-graphique"] if carte else []),
        stdout=journal, stderr=subprocess.STDOUT, stdin=subprocess.DEVNULL)
    return p, journal


def finir_telechargement(p, journal):
    print("\n  La voix et l'oreille…", flush=True)
    code = p.wait()
    journal.close()
    with open(os.path.join(EVE, "telechargement.log"), encoding="utf-8", errors="replace") as f:
        lignes = [l.rstrip() for l in f if l.strip()]
    for l in lignes[-9:]:
        print("    " + l)
    if code != 0:
        print("  Elles ne sont pas toutes là. Le lanceur reprendra le téléchargement au premier lancement.")
        return False
    # Piper et le serveur de la carte ont besoin du runtime Visual C++, qu'un poste n'a pas toujours : celui du kit.
    for dossier in (os.path.join(COMMUN, "piper", "piper"), os.path.join(COMMUN, "whisper-gpu")):
        if os.path.isdir(dossier):
            for dll in PROGRAMME_FICHIERS[1:]:
                shutil.copy2(os.path.join(PROGRAMME, dll), os.path.join(dossier, dll))
    return True


def privileges(choix):
    """Ce que Claude peut faire pour ce spécialiste, écrit en entier dans ses réglages : chaque privilège s'y retire ou
    s'y redonne à la main. La liberté totale, ou ses fichiers et le Web seulement ; Constructo AI ; les accords à la
    voix (faux : Eve ne demande plus rien, et son journal note ce qui l'aurait demandé)."""
    if choix["liberte"]:
        p = {"connecteurs": True, "chrome": True, "mcp_du_poste": True, "outils": ["tous"]}
    else:
        p = {"connecteurs": False, "chrome": False, "mcp_du_poste": False,
             "outils": ["Read", "Write", "Edit", "Glob", "Grep", "WebSearch", "WebFetch"]}
    p["constructo"] = bool(choix["constructo"])
    p["accords"] = bool(choix["accords"])
    return p


def jonction(lien, cible):
    """Un lien de dossier (sans droit d'administrateur) : chaque spécialiste voit la voix et l'oreille de commun\\."""
    if os.path.lexists(lien):
        return
    os.makedirs(cible, exist_ok=True)
    # cmd écrit dans la page de codes de la console (OEM), pas en UTF-8 : lu en UTF-8, « Jonction créée » faisait
    # planter le fil de lecture (une trace à l'écran) et un échec perdait son message.
    r = subprocess.run(["cmd", "/c", "mklink", "/J", lien, cible], capture_output=True, text=True, encoding="oem",
                       errors="replace")
    if r.returncode != 0:
        raise Echec(f"Le lien {lien} -> {cible} n'a pas pu se faire : {(r.stderr or r.stdout).strip()}")


def raccourci(nom_lnk, cible, dossier, icone):
    """Un raccourci sur le bureau, par PowerShell (les chemins passent par l'environnement : ni guillemets ni accents
    à échapper)."""
    script = (
        "$b = [Environment]::GetFolderPath('Desktop'); "
        "$s = (New-Object -ComObject WScript.Shell).CreateShortcut((Join-Path $b $env:EVE_LNK)); "
        "$s.TargetPath = $env:EVE_CIBLE; $s.WorkingDirectory = $env:EVE_REPERTOIRE; "
        "$s.IconLocation = $env:EVE_ICONE + ',0'; $s.WindowStyle = 7; $s.Save()"
    )
    env = dict(os.environ, EVE_LNK=nom_de_fichier(nom_lnk) + ".lnk", EVE_CIBLE=cible, EVE_REPERTOIRE=dossier,
               EVE_ICONE=icone)
    # Sa sortie ne sert pas : gardée en octets, jamais décodée (PowerShell écrit dans la page de codes OEM).
    r = subprocess.run(["powershell", "-NoProfile", "-NonInteractive", "-Command", script], env=env,
                       capture_output=True)
    if r.returncode != 0:
        print(f"  Le raccourci « {nom_lnk} » n'a pas pu se faire ; le lanceur est {cible}.")


# ---------------------------------------------------------------------------------------------------------------------
# L'écriture du profil
# ---------------------------------------------------------------------------------------------------------------------
def texte_d_entrevue(echanges):
    return "\n".join(f"- Question {i} : {e['question']}\n  Réponse : {e['reponse']}" for i, e in enumerate(echanges, 1))


def decrire(bloc, dossier):
    entree = bloc.get("input") or {}
    nom = bloc.get("name", "")
    if nom == "WebSearch":
        return "cherche : " + str(entree.get("query", ""))[:90]
    if nom == "WebFetch":
        url = str(entree.get("url", ""))
        return "lit : " + (re.sub(r"^https?://", "", url).split("/")[0] or url)[:90]
    if nom in ("Write", "Edit"):
        f = os.path.basename(str(entree.get("file_path", "")))
        taille = 0
        try:
            taille = os.path.getsize(os.path.join(dossier, f))
        except OSError:
            pass
        return f"écrit {f}" + (f" (déjà {taille // 1000} ko)" if taille else "")
    if nom == "Read":
        return "relit " + os.path.basename(str(entree.get("file_path", "")))
    return nom


def rediger(claude, dossier, consigne):
    """Un tour de Claude Code dans le dossier du spécialiste : il cherche sur le Web et écrit les fichiers ; chaque
    outil qu'il prend s'affiche, pour qu'on le voie travailler."""
    args = [claude, "-p", "--model", REDACTION[0], "--effort", REDACTION[1], "--tools",
            "Write,Edit,Read,WebSearch,WebFetch", "--dangerously-skip-permissions", "--strict-mcp-config",
            "--setting-sources", "", "--no-session-persistence", "--output-format", "stream-json", "--verbose"]
    p = subprocess.Popen(args, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
                         encoding="utf-8", errors="replace", cwd=dossier, env=env_claude())
    erreurs = []
    threading.Thread(target=lambda: erreurs.extend(p.stderr), daemon=True).start()
    trop_long = threading.Timer(REDACTION_MAX, p.kill)
    trop_long.start()
    fin = None
    try:
        p.stdin.write(consigne)
        p.stdin.close()
        for ligne in p.stdout:
            try:
                e = json.loads(ligne)
            except ValueError:
                continue
            if e.get("type") == "assistant":
                for bloc in (e.get("message") or {}).get("content") or []:
                    if isinstance(bloc, dict) and bloc.get("type") == "tool_use":
                        print("    · " + decrire(bloc, dossier), flush=True)
            elif e.get("type") == "result":
                fin = e
        p.wait()
    finally:
        trop_long.cancel()
    if not fin:
        detail = [l.strip() for l in erreurs if l.strip()]
        raise Echec("L'écriture du profil s'est arrêtée : " + (detail[-1] if detail else f"code {p.returncode}"))
    if fin.get("is_error"):
        raise Echec("L'écriture du profil a échoué : " + str(fin.get("result") or fin.get("subtype")))
    return fin


def ecrire_profil(claude, dossier, choix, echanges):
    print(f"\n  L'ÉCRITURE DU PROFIL : {REDACTION[0]} cherche sur le Web et écrit. Comptez de 5 à 20 minutes.")
    t0 = time.time()
    consigne = gabarit("redaction.md", NOM=choix["nom"], QUI=choix["prenom"] or "la personne", DATE=aujourdhui(),
                       ENTREVUE=texte_d_entrevue(echanges))
    couts = [rediger(claude, dossier, consigne).get("total_cost_usd") or 0]
    profil = os.path.join(dossier, "PROFIL.md")
    taille = len(open(profil, encoding="utf-8").read()) if os.path.isfile(profil) else 0
    if taille < PROFIL_MIN:
        print(f"\n  Le profil ne fait que {nombre(taille)} caractères : une seconde passe le complète.")
        suite = (f"PROFIL.md fait {taille} caractères : c'est trop mince pour un profil complet. Relis la consigne "
                 "ci-dessous, lis PROFIL.md s'il existe, puis complète-le avec Edit, section par section, jusqu'à au moins "
                 "30 000 caractères, le savoir de référence surtout. Écris aussi profil.json s'il manque.\n\n" + consigne)
        couts.append(rediger(claude, dossier, suite).get("total_cost_usd") or 0)
        taille = len(open(profil, encoding="utf-8").read()) if os.path.isfile(profil) else 0
    if not taille:
        raise Echec("Le profil n'a pas été écrit.")
    minutes = (time.time() - t0) / 60
    print(f"\n  Profil écrit : {nombre(taille)} caractères, en {minutes:.0f} min, pour environ {montant(sum(couts))} $ "
          "d'utilisation de ton compte Claude.")
    try:
        with open(os.path.join(dossier, "profil.json"), encoding="utf-8") as f:
            meta = json.load(f)
    except (OSError, ValueError):
        meta = {}
    resume = " ".join(str(meta.get("resume") or f"Tu es {choix['nom']}.").split())
    vocabulaire = [" ".join(str(v).split()) for v in (meta.get("vocabulaire") or []) if str(v).strip()][:40]
    return resume, vocabulaire


# ---------------------------------------------------------------------------------------------------------------------
# L'ensemble
# ---------------------------------------------------------------------------------------------------------------------
def installer(reponses):
    print("\n  EVE — un spécialiste à ta voix")
    print("  ==============================")
    print("  Gracieuseté de Sylvain Leduc — Constructo AI. Logiciel libre (Apache 2.0).")
    claude = verifier_claude()
    echanges, nom_suggere = entrevue(claude, reponses)
    choix = questions_communes(reponses, nom_suggere)

    print("\n  EN RÉSUMÉ")
    print(f"    Spécialiste : {choix['nom']}   (pour {choix['prenom'] or 'toi'})")
    print(f"    Dossier     : {choix['dossier']}")
    print(f"    Cerveau     : {choix['cerveau']}")
    print(f"    Pour parler : tenir {choix['touche_nom']}" + ("   ou dire « Eve, … »" if choix["eveil"] else ""))
    if choix["carte"]:
        print("    Oreille     : sur la carte graphique")
    print("    Privilèges  : " + ("la liberté totale" if choix["liberte"] else "ses fichiers et le Web"))
    print("    Constructo  : " + ("reliée au premier lancement" if choix["constructo"] else "non"))
    print("    Accords     : " + ("« je confirme » avant de supprimer ou d'écrire dans Constructo AI" if choix["accords"]
                                  else "aucun : tout part sans demander"))
    print(f"    Allure      : {choix['theme_nom']}")
    if not oui_non(reponses, "go", "On y va ?", True):
        raise Echec("Rien n'a été posé.")

    dossier = choix["dossier"]
    os.makedirs(dossier, exist_ok=True)
    ecrire(os.path.join(dossier, "entrevue.json"), json.dumps(
        {"date": datetime.datetime.now().isoformat(timespec="seconds"), "nom": choix["nom"], "echanges": echanges},
        ensure_ascii=False, indent=2) + "\n")
    poser_programme()
    telechargement = lancer_telechargement(choix["carte"])
    try:
        resume, vocabulaire = ecrire_profil(claude, dossier, choix, echanges)
    finally:
        voix_ok = finir_telechargement(*telechargement)

    ecrire(os.path.join(dossier, "CLAUDE.md"),
           gabarit("claude.md", NOM=choix["nom"], QUI=choix["prenom"] or "la personne", DATE=aujourdhui()))
    ecrire(os.path.join(dossier, "EVE_CONSIGNE.md"), gabarit("consigne_vocale.md", NOM=choix["nom"], RESUME=resume))
    os.makedirs(os.path.join(dossier, "scripts"), exist_ok=True)
    lisez = os.path.join(dossier, "scripts", "LISEZ-MOI.txt")
    if not os.path.exists(lisez):
        ecrire(lisez, f"Les scripts qu'Eve ({choix['nom']}) écrit pour toi.\n")
    lanceur = os.path.join(dossier, "Eve.bat")
    ecrire_bat(lanceur, gabarit("eve.bat", ID=choix["id"], DATE=datetime.date.today().isoformat()))

    local = os.path.join(SPECIALISTES, choix["id"])
    os.makedirs(local, exist_ok=True)
    # L'oreille de la carte : même si son téléchargement a échoué, la jonction dit qu'elle est voulue, et Eve.bat la
    # reprend au lancement ; sans elle, Eve écoute par le processeur.
    for d in ("modeles", "voix", "piper") + (("whisper-gpu",) if choix["carte"] else ()):
        jonction(os.path.join(local, d), os.path.join(COMMUN, d))
    mots = ", ".join(dict.fromkeys(["Eve", choix["nom"]] + vocabulaire))
    reglages = {
        "nom": choix["nom"], "qui": choix["prenom"], "modele": choix["modele"],
        "effort": choix["effort"],
        "port": choix["port"], "touche": choix["touche"], "eveil": choix["eveil"], "vocabulaire": mots[:700],
        # Pas de tour silencieux toutes les cinquante minutes : chacun se paie sur le compte de la personne.
        "au_chaud": False,
        # La fenêtre native (le centre de commande, l'orbe, la conversation) ; celle d'Edge la remplace sans OpenGL.
        "fenetre_native": True,
        "privileges": privileges(choix),
    }
    if choix["theme"]:
        reglages["theme"] = choix["theme"]
    ecrire(os.path.join(local, "reglages.json"), json.dumps(reglages, ensure_ascii=False, indent=2) + "\n")

    liste = [s for s in registre() if s.get("id") != choix["id"]]
    liste.append({"nom": choix["nom"], "id": choix["id"], "dossier": dossier, "port": choix["port"],
                  "touche": choix["touche"], "eveil": choix["eveil"], "pose_le": datetime.date.today().isoformat()})
    ecrire(REGISTRE, json.dumps(liste, ensure_ascii=False, indent=2) + "\n")

    if choix["raccourci"] and reponses.essai is None:
        raccourci(f"Eve — {choix['nom']}", lanceur, dossier, os.path.join(PROGRAMME, "eve.exe"))
        if os.path.isfile(os.path.join(EVE, "Installer.bat")):
            raccourci("Eve — nouveau spécialiste", os.path.join(EVE, "Installer.bat"), EVE,
                      os.path.join(PROGRAMME, "eve.exe"))

    print(f"\n  C'EST PRÊT : Eve — {choix['nom']}.")
    print(f"    Pour lui parler : tiens {choix['touche_nom']}" + (" ou dis « Eve, … »" if choix["eveil"] else "")
          + ". Ctrl+Alt+E affiche ou cache sa fenêtre.")
    print(f"    Son profil : {os.path.join(dossier, 'PROFIL.md')} (tu peux le modifier).")
    print("    Pour un autre spécialiste : relance Installer.bat (le raccourci « Eve — nouveau spécialiste »).")
    if not voix_ok:
        print("    La voix et l'oreille finiront de se télécharger au premier lancement.")
    if choix["constructo"]:
        print("    Constructo AI : dans ses réglages, « Se connecter à Constructo AI » relie Eve à ton entreprise ; "
              "« Se déconnecter » efface sa connexion.")
    if reponses.essai is None:
        print("\n  Eve démarre…")
        if choix["constructo"]:
            # Eve ouvre d'elle-même la page de connexion de Constructo AI, à ce premier lancement seulement : la variable
            # ne vit que pour lui (le lanceur la passe au programme ; les suivants ne l'ont pas).
            os.environ["EVE_RELIER_CONSTRUCTO"] = "1"
            print("  La page de connexion de Constructo AI va s'ouvrir dans ton navigateur : entre le courriel et le mot "
                  "de passe de ton entreprise, puis reviens à la fenêtre d'Eve.")
        os.startfile(lanceur)
    return 0


def main():
    ap = argparse.ArgumentParser(description="L'installateur d'Eve : un spécialiste bâti par une entrevue.")
    ap.add_argument("--essai", metavar="REPONSES.json", help="les réponses viennent du fichier ; rien ne se lance")
    a = ap.parse_args()
    try:
        return installer(Reponses(a.essai))
    except Echec as e:
        print(f"\n  {e}")
        return 1
    except KeyboardInterrupt:
        print("\n\n  Installation interrompue.")
        return 1


if __name__ == "__main__":
    sys.exit(main())
