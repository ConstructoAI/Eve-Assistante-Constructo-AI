@echo off
title Eve - l'appli Android
REM Construit l'appli Android d'Eve : app\build\outputs\apk\debug\app-debug.apk, dans ce dossier.
REM
REM   Construire.bat               construit l'appli
REM   Construire.bat --essais      construit, puis lance les essais : gradlew testDebugUnitTest
REM   Construire.bat --sans-pause  ne s'arrete pas a la fin
REM
REM Ce qu'il faut sur le poste : Android Studio, pour son Java - le dossier jbr - et son SDK, que Studio pose dans
REM %LOCALAPPDATA%\Android\Sdk avec les licences acceptees ; la plateforme qui manque, la construction la telecharge.
REM JAVA_HOME et ANDROID_HOME, s'ils sont poses, passent avant.
REM Gradle vient seul, par gradlew, verifie par son SHA-256 - gradle\wrapper\gradle-wrapper.properties.
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
if not defined JAVA_HOME if exist "%ProgramFiles%\Android\Android Studio\jbr\bin\java.exe" set "JAVA_HOME=%ProgramFiles%\Android\Android Studio\jbr"
if not defined JAVA_HOME goto sans_java
if not defined ANDROID_HOME if exist "%LOCALAPPDATA%\Android\Sdk\platforms" set "ANDROID_HOME=%LOCALAPPDATA%\Android\Sdk"
if not defined ANDROID_HOME goto sans_sdk
set "TACHES=assembleDebug"
if defined ESSAIS set "TACHES=assembleDebug testDebugUnitTest"
REM Par son chemin : sous NoDefaultCurrentDirectoryInExePath, cmd ne cherche rien dans le dossier courant.
call "%~dp0gradlew.bat" %TACHES%
if errorlevel 1 goto echec
echo.
echo   L'appli est construite : %~dp0app\build\outputs\apk\debug\app-debug.apk
echo.
if defined ARRET pause
exit /b 0

:sans_java
echo.
echo   Pas de Java : installer Android Studio, https://developer.android.com/studio, ou poser JAVA_HOME vers un JDK 17
echo   ou plus, puis relancer Construire.bat.
echo.
if defined ARRET pause
exit /b 1

:sans_sdk
echo.
echo   Pas de SDK d'Android : ouvrir Android Studio une fois - il le pose, licences comprises - ou poser ANDROID_HOME
echo   vers lui, puis relancer Construire.bat.
echo.
if defined ARRET pause
exit /b 1

:echec
echo.
echo   La construction a echoue : voir les messages ci-dessus.
echo.
if defined ARRET pause
exit /b 1
