# Prints the UI Automation tree of the Dromaius window, i.e. what a
# screen reader sees, plus the element that currently has keyboard focus.
# Run with Windows PowerShell 5.1 (powershell.exe), which ships the UIA client.
#
#   powershell -ExecutionPolicy Bypass -File tools\uia-dump.ps1 [-Depth 6] [-Keys "{TAB}"]

param(
    [int]$Depth = 8,
    [int]$ProcessId = 0,
    [string]$Keys = "",
    [int]$WaitMs = 600
)

Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Windows.Forms
$ae = [System.Windows.Automation.AutomationElement]

$proc = if ($ProcessId) { Get-Process -Id $ProcessId } else { Get-Process dromaius -ErrorAction SilentlyContinue | Select-Object -First 1 }
if (-not $proc) { Write-Error "dromaius is not running"; exit 1 }
$win = $ae::RootElement.FindFirst(
    [System.Windows.Automation.TreeScope]::Children,
    (New-Object System.Windows.Automation.PropertyCondition($ae::ProcessIdProperty, $proc.Id)))
if (-not $win) { Write-Error "window not found"; exit 1 }

if ($Keys) {
    Add-Type @"
using System; using System.Runtime.InteropServices;
public static class Win { [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h); }
"@
    [Win]::SetForegroundWindow([IntPtr]$win.Current.NativeWindowHandle) | Out-Null
    Start-Sleep -Milliseconds 300
    foreach ($k in $Keys -split '\|') {
        [System.Windows.Forms.SendKeys]::SendWait($k)
        Start-Sleep -Milliseconds $WaitMs
    }
}

function Describe($e) {
    $c = $e.Current
    $s = "$($c.ControlType.ProgrammaticName -replace 'ControlType\.','') '$($c.Name)'"
    $vp = $null
    if ($e.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern, [ref]$vp)) {
        $s += " value='$($vp.Current.Value)'"
    }
    $tp = $null
    if ($e.TryGetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern, [ref]$tp)) {
        $s += " toggle=$($tp.Current.ToggleState)"
    }
    if ($c.HelpText) { $s += " help='$($c.HelpText)'" }
    if ($c.HasKeyboardFocus) { $s += "  <== FOCUS" }
    $s
}

function Walk($e, $level) {
    if ($level -gt $Depth) { return }
    Write-Output (("  " * $level) + (Describe $e))
    $walker = [System.Windows.Automation.TreeWalker]::ControlViewWalker
    $child = $walker.GetFirstChild($e)
    while ($child) {
        Walk $child ($level + 1)
        $child = $walker.GetNextSibling($child)
    }
}

Walk $win 0
$f = $ae::FocusedElement
Write-Output "--- focused: $(Describe $f)"
$tp = $null
if ($f.TryGetCurrentPattern([System.Windows.Automation.TextPattern]::Pattern, [ref]$tp)) {
    $sel = $tp.GetSelection()
    if ($sel.Length -gt 0) {
        $r = $sel[0].Clone()
        $r.ExpandToEnclosingUnit([System.Windows.Automation.Text.TextUnit]::Character)
        Write-Output "--- text: '$($tp.DocumentRange.GetText(-1))' caret-char: '$($r.GetText(-1))'"
    }
}
