@echo off
setlocal
REM Eve - l'installateur. Une entrevue batit, question par question, le profil complet d'un
REM specialiste, puis Eve devient ce specialiste. Relancer pour en ajouter un autre.
REM
REM Aucun caractere accentue dans ce fichier : la page de codes de cmd.exe change selon le mode de
REM lancement. Fins de ligne CRLF obligatoires. Pas de parenthese dans un echo.

cd /d "%~dp0"
if not exist "programme\python\python.exe" goto :incomplet
if not exist "programme\outils\installer.py" goto :incomplet
"programme\python\python.exe" -X utf8 -s "programme\outils\installer.py" %*
set "CODE=%ERRORLEVEL%"
echo.
pause
exit /b %CODE%

:incomplet
echo.
echo   Le kit d'Eve est incomplet : le dossier programme manque, ou une partie.
echo   Decompresser l'archive en entier, puis relancer Installer.bat.
echo.
pause
exit /b 1
