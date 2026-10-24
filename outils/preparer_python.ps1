# Prepare le Python du kit d'Eve : Python 3.11.9 "embarquable" (python.org), sans rien installer sur le poste. Il fait
# tourner l'installateur (outils\installer.py) et le telechargeur de la voix (outils\ressources_eve.py), qui n'emploient
# que la bibliotheque standard : la personne qui recoit Eve n'a pas a installer Python. Le resultat va dans python\ (hors
# git) ; emballer.ps1 le copie dans programme\python du kit.
#
#   powershell -ExecutionPolicy Bypass -File preparer_python.ps1
#
# Le telechargement est verifie a son SHA-256 (le meme que celui d'Eve pour Constructo AI, releve le 2026-10-22).
# Ce fichier reste en ASCII : PowerShell 5.1 lit un script sans BOM dans la page de codes du poste.
$ErrorActionPreference = "Stop"
$cible = Join-Path $PSScriptRoot "python"
$zipUrl = "https://www.python.org/ftp/python/3.11.9/python-3.11.9-embed-amd64.zip"
$zipSha = "009d6bf7e3b2ddca3d784fa09f90fe54336d5b60f0e0f305c37f400bf83cfd3b"
$zip = Join-Path $env:TEMP ("eve-python-" + [guid]::NewGuid().ToString("N") + ".zip")
try {
  [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
  Invoke-WebRequest -UseBasicParsing -Uri $zipUrl -OutFile $zip
  $h = (Get-FileHash -Algorithm SHA256 $zip).Hash.ToLower()
  if ($h -ne $zipSha) { throw "Python embarquable : SHA-256 inattendu ($h)" }
  if (Test-Path $cible) { Remove-Item -Recurse -Force $cible }
  New-Item -ItemType Directory -Force $cible | Out-Null
  Add-Type -AssemblyName System.IO.Compression.FileSystem
  [IO.Compression.ZipFile]::ExtractToDirectory($zip, $cible)
  # La preuve : ce Python-la charge ce dont l'installateur et le telechargeur ont besoin.
  & (Join-Path $cible "python.exe") -X utf8 -c "import json, ssl, urllib.request, hashlib, zipfile, socket, tempfile, threading, unicodedata; print('Python du kit pret')"
  if ($LASTEXITCODE -ne 0) { throw "le Python embarque ne charge pas la bibliotheque standard" }
  "Python du kit : {0} ({1:N1} Mo)" -f $cible, ((Get-ChildItem -Recurse -File $cible | Measure-Object Length -Sum).Sum / 1MB)
} finally {
  Remove-Item -Force $zip -ErrorAction SilentlyContinue
}
