@echo off
title Eve - compilation
REM Compile Eve : target\release\eve.exe, dans ce dossier. emballer.ps1 en fait ensuite le kit, dist\Eve.zip.
REM
REM   Compiler.bat               compile le programme
REM   Compiler.bat --essais      compile, puis lance les essais : cargo test --release
REM   Compiler.bat --sans-pause  ne s'arrete pas a la fin : pour emballer.ps1
REM
REM Ce qu'il faut sur le poste : Rust, par rustup, la version de rust-toolchain.toml se pose seule ; Visual Studio 2022
REM avec les outils C++ ; CMake ; une libclang pour whisper-rs - le paquet PyPI libclang 18.1.1, sa libclang.dll seule -
REM deposee dans outils\libclang, que .gitignore ecarte du depot, ou nommee par la variable LIBCLANG_PATH.
REM Aucun caractere accentue dans ce fichier, fins de ligne CRLF, pas de parenthese dans un echo.
setlocal
cd /d "%~dp0"
set "ARRET=1"
set "ESSAIS="
:arguments
if "%~1"=="" goto arguments_lus
if /i "%~1"=="--sans-pause" set "ARRET="
if /i "%~1"=="--essais" set "ESSAIS=1"
shift
goto arguments
:arguments_lus
REM Le programme publie ne porte pas le nom de session du poste qui le compile : les chemins des sources, gardes pour
REM les messages d'erreur, commencent par ~ au lieu du profil. CL remplace celui de .cargo\config.toml : il en garde
REM le /D__F16C__ - voir ce fichier.
set "RUSTFLAGS=--remap-path-prefix=%USERPROFILE%=~"
set "CL=/D__F16C__ /d1trimfile:%USERPROFILE%\"
REM L'empreinte de la source - le commit, suivi de -modifie si l'arbre a change depuis - entre dans le programme :
REM il la note au journal a chaque demarrage, et eve --mesures range ainsi ses mesures par version. Sans git, rien.
REM Dans la commande du for, le signe egal s'ecrit ^= : nu, cmd en fait un separateur et git ne rend rien.
set "EVE_EMPREINTE="
for /f "delims=" %%e in ('git describe --always --dirty^=-modifie 2^>nul') do set "EVE_EMPREINTE=%%e"
if defined LIBCLANG_PATH goto libclang_prete
if not exist "%~dp0outils\libclang\libclang.dll" goto sans_libclang
set "LIBCLANG_PATH=%~dp0outils\libclang"
:libclang_prete
where cargo >nul 2>nul
if errorlevel 1 if exist "%USERPROFILE%\.cargo\bin\cargo.exe" set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
where cargo >nul 2>nul
if errorlevel 1 goto sans_rust
cargo build --release
if errorlevel 1 goto echec
if not defined ESSAIS goto compile
cargo test --release
if errorlevel 1 goto echec_essais
:compile
echo.
echo   Eve est compilee : %~dp0target\release\eve.exe
echo.
if defined ARRET pause
exit /b 0

:sans_libclang
echo.
echo   La libclang manque : deposer libclang.dll dans outils\libclang de ce dossier - le paquet PyPI libclang 18.1.1 -
echo   ou poser la variable LIBCLANG_PATH vers son dossier, puis relancer Compiler.bat.
echo.
if defined ARRET pause
exit /b 1

:sans_rust
echo.
echo   Rust n'est pas installe sur ce poste : l'installer par rustup, https://rustup.rs, puis relancer Compiler.bat.
echo   Il faut aussi Visual Studio 2022, avec les outils C++, et CMake.
echo.
if defined ARRET pause
exit /b 1

:echec
echo.
echo   La compilation a echoue : voir les messages ci-dessus.
echo.
if defined ARRET pause
exit /b 1

:echec_essais
echo.
echo   Des essais ont echoue : voir les messages ci-dessus.
echo.
if defined ARRET pause
exit /b 1
