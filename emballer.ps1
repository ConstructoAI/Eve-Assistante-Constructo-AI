# Emballe Eve a partager : dist\Eve.zip (le programme et le runtime Visual C++, son Python, son installateur et ses
# gabarits, le telechargeur de la voix, le guide et les licences) et l'empreinte SHA-256 de celle-ci. La voix et
# l'oreille - environ 410 Mo - n'y sont pas : l'installateur les telecharge, de leurs sources publiques officielles.
#
#   powershell -ExecutionPolicy Bypass -File emballer.ps1 [-Construire] [-Sortie <dossier>]
#
# Tout vient de ce dossier, C:\Dev\Eve : le programme de target\release\eve.exe (-Construire le recompile d'abord, par
# Compiler.bat), ce qui part chez la personne de kit\, le Python embarquable de outils\python (preparer_python.ps1).
# -Sortie : ou ecrire l'archive (par defaut dist\, a cote de ce fichier). Le kit se prepare dans <Sortie>\paquet\Eve.
#
# Ce fichier reste en ASCII : PowerShell 5.1 lit un script sans BOM dans la page de codes du poste.
param(
  [switch]$Construire,
  [string]$Sortie = ""
)
$ErrorActionPreference = "Stop"
$racine = $PSScriptRoot
$kit = Join-Path $racine "kit"
if (-not $Sortie) { $Sortie = Join-Path $racine "dist" }

# 1. Le programme.
if ($Construire) {
  & cmd /c "`"$(Join-Path $racine 'Compiler.bat')`" --sans-pause"
  if ($LASTEXITCODE -ne 0) { throw "la construction a echoue" }
}
$exe = Join-Path $racine "target\release\eve.exe"
if (-not (Test-Path $exe)) { throw "$exe manque : relancer avec -Construire" }

# 2. Ce qu'il faut emporter, verifie avant de rien ecrire.
$python = Join-Path $racine "outils\python"
if (-not (Test-Path (Join-Path $python "python.exe"))) { throw "outils\python manque : lancer d'abord outils\preparer_python.ps1" }
# Visual Studio 2022 dans Program Files, ses Build Tools dans Program Files (x86) : le LISEZ-MOI des sources dit l'un ou
# l'autre.
$vs = @($env:ProgramFiles, ${env:ProgramFiles(x86)}) | Where-Object { $_ } |
  ForEach-Object { Join-Path $_ "Microsoft Visual Studio\2022\*\VC\Redist\MSVC\*\x64\Microsoft.VC143.CRT" }
$crt = Get-ChildItem $vs -Directory -ErrorAction SilentlyContinue | Sort-Object FullName | Select-Object -Last 1
if (-not $crt) { throw "le runtime Visual C++ redistribuable (Microsoft.VC143.CRT) est introuvable : installer Visual Studio 2022 (C++)" }
$dlls = @("msvcp140.dll", "vcruntime140.dll", "vcruntime140_1.dll") | ForEach-Object { Join-Path $crt.FullName $_ }
foreach ($d in $dlls) { if (-not (Test-Path $d)) { throw "runtime introuvable : $d" } }
$gabarits = @("entrevue.md", "redaction.md", "consigne_vocale.md", "claude.md", "eve.bat")
foreach ($t in @("Installer.bat", "LISEZ-MOI.txt", "LICENCES-TIERS.txt", "outils\installer.py", "outils\ressources_eve.py") +
  ($gabarits | ForEach-Object { "outils\gabarits\$_" })) {
  if (-not (Test-Path (Join-Path $kit $t))) { throw "kit\$t manque" }
}
# Le lanceur de chaque specialiste est ecrit en ASCII par l'installateur : son gabarit doit l'etre deja.
foreach ($t in @("Installer.bat", "outils\gabarits\eve.bat")) {
  $o = [IO.File]::ReadAllBytes((Join-Path $kit $t))
  if (@($o | Where-Object { $_ -gt 127 }).Count) { throw "kit\$t : caractere non ASCII (cmd.exe l'afficherait en charabia)" }
}

# 3. Le dossier "Eve", neuf.
$prep = Join-Path $Sortie "paquet"
$eve = Join-Path $prep "Eve"
if (Test-Path $prep) { Remove-Item -Recurse -Force $prep }
$prog = Join-Path $eve "programme"
New-Item -ItemType Directory -Force (Join-Path $prog "outils\gabarits") | Out-Null
Copy-Item $exe (Join-Path $prog "eve.exe")
foreach ($d in $dlls) { Copy-Item $d $prog }
Copy-Item -Recurse $python (Join-Path $prog "python")
Copy-Item (Join-Path $kit "outils\installer.py"), (Join-Path $kit "outils\ressources_eve.py") (Join-Path $prog "outils")
foreach ($g in $gabarits) { Copy-Item (Join-Path $kit "outils\gabarits\$g") (Join-Path $prog "outils\gabarits") }
# Installer.bat en CRLF : cmd.exe perd ses etiquettes en LF, quoi que git en ait fait.
$bat = [IO.File]::ReadAllText((Join-Path $kit "Installer.bat")) -replace "`r?`n", "`r`n"
[IO.File]::WriteAllText((Join-Path $eve "Installer.bat"), $bat, (New-Object Text.ASCIIEncoding))
# Les textes de la personne, avec un BOM : le Bloc-notes d'un vieux Windows 10 les lit alors en UTF-8.
$avecBom = New-Object Text.UTF8Encoding $true
foreach ($t in @("LISEZ-MOI.txt", "LICENCES-TIERS.txt")) {
  $texte = [IO.File]::ReadAllText((Join-Path $kit $t), [Text.Encoding]::UTF8) -replace "`r?`n", "`r`n"
  [IO.File]::WriteAllText((Join-Path $eve $t), $texte, $avecBom)
}

# 4. Les bibliotheques Rust et leurs licences, lues dans Cargo.lock (cargo metadata) au moment de l'emballage.
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
Push-Location $racine
try {
  $meta = cargo metadata --format-version 1 --filter-platform x86_64-pc-windows-msvc | ConvertFrom-Json
} finally { Pop-Location }
$dans = @{}; foreach ($n in $meta.resolve.nodes) { $dans[$n.id] = $true }
$lignes = $meta.packages | Where-Object { $dans.ContainsKey($_.id) -and $_.name -ne "eve" } | Sort-Object name |
  ForEach-Object { "{0} {1} : {2}" -f $_.name, $_.version, $_.license }
$entete = "Les bibliotheques Rust du programme d'Eve et de sa construction (Cargo.lock), avec la licence de chacune.`r`n" +
  "Les textes des licences : https://spdx.org/licenses/`r`n`r`n"
[IO.File]::WriteAllText((Join-Path $eve "LICENCES-RUST.txt"), $entete + ($lignes -join "`r`n") + "`r`n", $avecBom)

# 4 bis. La licence d'Eve (Apache 2.0) et sa NOTICE, a la racine ; les sources, pour que chacun puisse lire, changer et
# reconstruire Eve. Rien de ce qui parle de la personne qui la developpe : ni ETAT.md, ni structure.md, ni CLAUDE.md,
# ni docs\, ni l'historique git.
foreach ($t in @("LICENSE", "NOTICE")) {
  $texte = [IO.File]::ReadAllText((Join-Path $racine $t), [Text.Encoding]::UTF8) -replace "`r?`n", "`r`n"
  [IO.File]::WriteAllText((Join-Path $eve $t), $texte, $avecBom)
}
$sources = Join-Path $eve "sources"
$fichiers = @("Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "rustfmt.toml", ".cargo\config.toml", "Compiler.bat",
  "emballer.ps1", "LICENSE", "NOTICE", ".gitattributes", ".gitignore", "outils\preparer_python.ps1",
  "outils\capture_essai.ps1")
foreach ($f in $fichiers) {
  $cible = Join-Path $sources $f
  New-Item -ItemType Directory -Force (Split-Path $cible) | Out-Null
  Copy-Item (Join-Path $racine $f) $cible
}
# essais\ ne part pas : la liste des outils du serveur de Constructo AI y decrit l'ERP ; l'essai qui la lit se passe
# d'elle (garde.rs).
foreach ($d in @("src", "page", "kit")) { Copy-Item -Recurse (Join-Path $racine $d) (Join-Path $sources $d) }
# Ce que Python compile quand on essaie l'installateur sur ce poste (__pycache__) ne part pas.
Get-ChildItem -Recurse -Directory -Force $sources -Filter "__pycache__" | Remove-Item -Recurse -Force
$texte = [IO.File]::ReadAllText((Join-Path $kit "LISEZ-MOI-SOURCES.txt"), [Text.Encoding]::UTF8) -replace "`r?`n", "`r`n"
[IO.File]::WriteAllText((Join-Path $sources "LISEZ-MOI.txt"), $texte, $avecBom)

# 5. L'archive : le dossier "Eve" a sa racine. Chaque entree est nommee a la main, avec des "/" : sous PowerShell 5.1,
# ZipFile.CreateFromDirectory ecrit des "\", contraires au format ZIP.
Add-Type -AssemblyName System.IO.Compression, System.IO.Compression.FileSystem
$zip = Join-Path $Sortie "Eve.zip"
if (Test-Path $zip) { Remove-Item -Force $zip }
$flux = [IO.File]::Open($zip, [IO.FileMode]::CreateNew)
try {
  $archive = New-Object IO.Compression.ZipArchive($flux, [IO.Compression.ZipArchiveMode]::Create)
  try {
    Get-ChildItem -Recurse -File $eve | Sort-Object FullName | ForEach-Object {
      $nom = "Eve/" + $_.FullName.Substring($eve.Length + 1).Replace("\", "/")
      [IO.Compression.ZipFileExtensions]::CreateEntryFromFile($archive, $_.FullName, $nom, [IO.Compression.CompressionLevel]::Optimal) | Out-Null
    }
  } finally { $archive.Dispose() }
} finally { $flux.Dispose() }
$empreinte = (Get-FileHash -Algorithm SHA256 $zip).Hash.ToLower()
[IO.File]::WriteAllText("$zip.sha256", "$empreinte  Eve.zip`n")

$taille = (Get-ChildItem -Recurse -File $eve | Measure-Object Length -Sum).Sum
"Programme : {0} ({1:yyyy-MM-dd HH:mm})" -f $exe, (Get-Item $exe).LastWriteTime
"Dossier   : {0} ({1:N0} Mo, {2} fichiers)" -f $eve, ($taille / 1MB), (Get-ChildItem -Recurse -File $eve).Count
"Archive   : {0} ({1:N1} Mo)" -f $zip, ((Get-Item $zip).Length / 1MB)
"SHA-256   : $empreinte"
