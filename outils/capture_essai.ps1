param([string]$Motif, [string]$Sortie, [int]$Largeur = 0, [int]$Hauteur = 0)
# Capture la fenetre d'un eve.exe dont le titre correspond au motif, par PrintWindow : seule cette fenetre est lue, sans
# la mettre devant. Refuse toute fenetre de l'Assistante (son titre porte son nom). Largeur et Hauteur :
# la redimensionner d'abord (sur place).
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class Prise {
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint f);
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr a, int x, int y, int l, int t, uint f);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  public struct RECT { public int L, T, R, B; }
}
"@
if ($Motif -notlike "*Essai*") { "motif refuse : il doit nommer l'Eve d'essai"; exit 2 }
[Prise]::SetProcessDPIAware() | Out-Null
Add-Type -AssemblyName System.Drawing
$p = Get-Process eve -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowTitle -like $Motif } | Select-Object -First 1
if (-not $p) { "fenetre introuvable"; exit 1 }
if ($p.MainWindowTitle -like "*Assistante*") { "refus : c'est la fenetre de l'Assistante"; exit 3 }
$h = $p.MainWindowHandle
if ($Largeur -gt 0) {
  # SWP_NOMOVE (2) | SWP_NOZORDER (4) | SWP_NOACTIVATE (0x10)
  [Prise]::SetWindowPos($h, [IntPtr]::Zero, 0, 0, $Largeur, $Hauteur, 0x16) | Out-Null
  Start-Sleep -Milliseconds 900
}
$r = New-Object Prise+RECT
[Prise]::GetWindowRect($h, [ref]$r) | Out-Null
$w = $r.R - $r.L
$hh = $r.B - $r.T
$bmp = New-Object System.Drawing.Bitmap $w, $hh
$g = [System.Drawing.Graphics]::FromImage($bmp)
$dc = $g.GetHdc()
# PW_RENDERFULLCONTENT (2) : le contenu dessine par la carte graphique compris.
[Prise]::PrintWindow($h, $dc, 2) | Out-Null
$g.ReleaseHdc($dc)
$bmp.Save($Sortie, [System.Drawing.Imaging.ImageFormat]::Png)
"capture $w x $hh : $($p.MainWindowTitle)"
