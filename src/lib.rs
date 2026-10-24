//! Eve — l'Adjointe de vive voix.
//!
//! La personne tient une touche et parle ; Eve transcrit (whisper.cpp, sur le poste), confie la demande à la même
//! Adjointe que lance `Adjointe.bat` (Claude Code, dans son dossier, avec son `CLAUDE.md`, ses scripts et le serveur
//! MCP d'Entrepreneur Général), puis lit la réponse à voix haute (les voix de Windows). Rien n'est refait à côté :
//! la voix n'est qu'une porte d'entrée et de sortie devant l'Adjointe.
//!
//! Cette bibliothèque est la partie pure, sans micro ni fenêtre, qui compile partout et pourra entrer dans
//! Entrepreneur Général :
//! - [`garde`] : ce qui demande l'accord de la personne avant de partir (et ce qui ne part jamais) ;
//! - [`mesures`] : le temps de chaque étape d'une demande et le coût de chaque tour, notés au journal et relus par
//!   `eve --mesures` ;
//! - [`parole`] : ce qui se dit à voix haute, et ce que la personne a voulu dire (« je confirme », « arrête ») ;
//! - [`pdf`] : les pages numérisées par la caméra, réunies en un PDF ;
//! - [`protocole`] : les messages qu'échangent Eve et Claude Code (`--input-format stream-json`, le protocole de
//!   contrôle des SDK officiels : l'initialisation, les crochets, les permissions, l'interruption).

pub mod annuler;
pub mod garde;
pub mod mesures;
pub mod parole;
pub mod pdf;
pub mod protocole;
