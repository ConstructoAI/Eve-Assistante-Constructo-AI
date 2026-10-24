@echo off
setlocal
REM Eve - un specialiste, pose par l'installateur d'Eve le {{DATE}}. Double-cliquer pour la lancer.
REM Son profil, sa consigne et ses notes sont dans ce dossier ; ses reglages, son journal et sa
REM memoire dans LOCALAPPDATA\Eve\specialistes\{{ID}} ; le programme, la voix et l'oreille, partages
REM par tous les specialistes, dans LOCALAPPDATA\Eve.
REM
REM Aucun caractere accentue dans ce fichier : la page de codes de cmd.exe change selon le mode de
REM lancement. Fins de ligne CRLF obligatoires. Pas de parenthese dans un echo.

cd /d "%~dp0"
set "EVE=%LOCALAPPDATA%\Eve"
set "PROGRAMME=%EVE%\programme"
set "LOCAL=%EVE%\specialistes\{{ID}}"
REM Un programme remplace pendant qu'une Eve tournait se retire des qu'il ne tourne plus.
del /q "%PROGRAMME%\*.remplace.*" >nul 2>nul
if not exist "%PROGRAMME%\eve.exe" goto :pas_de_programme
if not exist "%LOCAL%\reglages.json" goto :pas_de_reglages

REM La voix et l'oreille, telechargees une fois pour tous les specialistes : reprises ici si elles manquent.
if exist "%EVE%\commun\modeles\ggml-small-q5_1.bin" if exist "%EVE%\commun\piper\piper\piper.exe" goto :carte
"%PROGRAMME%\python\python.exe" -X utf8 -s "%PROGRAMME%\outils\ressources_eve.py" --installer --dossier "%EVE%\commun"
if errorlevel 1 goto :echec_ressources
for %%F in (msvcp140.dll vcruntime140.dll vcruntime140_1.dll) do copy /y "%PROGRAMME%\%%F" "%EVE%\commun\piper\piper\" >nul 2>nul

:carte
REM L'oreille sur la carte graphique, si ce specialiste l'a : reprise ici si elle manque. Un echec ne bloque rien :
REM Eve ecoute alors par le processeur.
if not exist "%LOCAL%\whisper-gpu" goto :lancer
if exist "%EVE%\commun\whisper-gpu\whisper-server.exe" goto :lancer
"%PROGRAMME%\python\python.exe" -X utf8 -s "%PROGRAMME%\outils\ressources_eve.py" --installer --carte-graphique --dossier "%EVE%\commun"
for %%F in (msvcp140.dll vcruntime140.dll vcruntime140_1.dll) do copy /y "%PROGRAMME%\%%F" "%EVE%\commun\whisper-gpu\" >nul 2>nul

:lancer
set "EVE_DOSSIER=%~dp0"
set "EVE_DOSSIER_LOCAL=%LOCAL%"
if not defined CLAUDE_CODE_USE_POWERSHELL_TOOL set "CLAUDE_CODE_USE_POWERSHELL_TOOL=1"
start "Eve {{ID}} - journal - ne pas fermer" /min "%PROGRAMME%\eve.exe"
exit /b 0

:pas_de_programme
echo.
echo   Le programme d'Eve manque dans %PROGRAMME%.
echo   Relancer Installer.bat, celui du kit d'Eve ou celui de %EVE%.
echo.
pause
exit /b 1

:pas_de_reglages
echo.
echo   Les reglages de ce specialiste manquent dans %LOCAL%.
echo   Refaire l'installation : Installer.bat, celui du kit d'Eve ou celui de %EVE%.
echo.
pause
exit /b 1

:echec_ressources
echo.
echo   La voix et l'oreille d'Eve ne sont pas toutes la : le message ci-dessus dit pourquoi.
echo   Verifier la connexion Internet, puis relancer : le telechargement reprend ou il s'etait arrete.
echo.
pause
exit /b 1
