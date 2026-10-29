# Les icones de l'appli Android, tirees du logo de Constructo AI (logo-constructo-ai.png, a cote de ce script, celui
# du site) : le logo en blanc, sa transparence gardee, comme l'icone de la fenetre d'Eve sur l'ordinateur.
#
#   powershell -ExecutionPolicy Bypass -File mobile\outils\icones.ps1
#
# Ce qu'il ecrit, sous app\src\main\res :
# - mipmap-*\ic_launcher_avant.png : le premier plan de l'icone adaptative, a chaque densite (108 dp de cote). Le logo
#   n'en prend que 42 pour cent, au milieu : un lanceur qui decoupe l'icone en cercle n'en garde que 66 dp, et les
#   coins du bas du logo doivent y tenir ;
# - drawable-xxxhdpi\logo_blanc.png : le logo seul, 32 dp de haut, pour la barre de l'appli.
# Aucun caractere accentue dans ce fichier (PowerShell 5.1 lit un .ps1 sans BOM dans la page de code du poste).
param([string]$Logo = (Join-Path $PSScriptRoot 'logo-constructo-ai.png'))
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

$res = Join-Path $PSScriptRoot '..\app\src\main\res'
$source = [System.Drawing.Bitmap]::FromFile((Resolve-Path $Logo))
try {
    # Le logo en blanc : chaque pixel garde sa transparence, son bleu et son noir deviennent du blanc.
    $blanc = New-Object System.Drawing.Bitmap $source.Width, $source.Height, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $zone = New-Object System.Drawing.Rectangle 0, 0, $source.Width, $source.Height
    $lu = $source.LockBits($zone, [System.Drawing.Imaging.ImageLockMode]::ReadOnly, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $octets = New-Object byte[] ($lu.Stride * $source.Height)
    [System.Runtime.InteropServices.Marshal]::Copy($lu.Scan0, $octets, 0, $octets.Length)
    $source.UnlockBits($lu)
    for ($i = 0; $i -lt $octets.Length; $i += 4) { $octets[$i] = 255; $octets[$i + 1] = 255; $octets[$i + 2] = 255 }
    $ecrit = $blanc.LockBits($zone, [System.Drawing.Imaging.ImageLockMode]::WriteOnly, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    [System.Runtime.InteropServices.Marshal]::Copy($octets, 0, $ecrit.Scan0, $octets.Length)
    $blanc.UnlockBits($ecrit)

    function Poser([string]$chemin, [int]$largeur, [int]$hauteur, [double]$part) {
        $image = New-Object System.Drawing.Bitmap $largeur, $hauteur, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
        $g = [System.Drawing.Graphics]::FromImage($image)
        $g.Clear([System.Drawing.Color]::Transparent)
        $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
        $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
        $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
        $echelle = [Math]::Min($largeur * $part / $blanc.Width, $hauteur * $part / $blanc.Height)
        $l = $blanc.Width * $echelle; $h = $blanc.Height * $echelle
        $g.DrawImage($blanc, [single](($largeur - $l) / 2), [single](($hauteur - $h) / 2), [single]$l, [single]$h)
        $g.Dispose()
        New-Item -ItemType Directory -Force (Split-Path $chemin) | Out-Null
        $image.Save($chemin, [System.Drawing.Imaging.ImageFormat]::Png)
        $image.Dispose()
        Write-Output ("{0} : {1} x {2}" -f $chemin.Substring($res.Length + 1), $largeur, $hauteur)
    }

    # Les densites d'Android : 1, 1,5, 2, 3 et 4 pixels par dp.
    foreach ($d in @(@('mdpi', 1.0), @('hdpi', 1.5), @('xhdpi', 2.0), @('xxhdpi', 3.0), @('xxxhdpi', 4.0))) {
        $cote = [int](108 * $d[1])
        Poser (Join-Path $res ("mipmap-{0}\ic_launcher_avant.png" -f $d[0])) $cote $cote 0.42
    }
    $hauteur = 32 * 4
    Poser (Join-Path $res 'drawable-xxxhdpi\logo_blanc.png') ([int][Math]::Ceiling($hauteur * $blanc.Width / $blanc.Height)) $hauteur 1.0
    $blanc.Dispose()
} finally {
    $source.Dispose()
}
