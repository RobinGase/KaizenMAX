Dim shell
Dim fso
Dim scriptDir
Dim command

Set shell = CreateObject("WScript.Shell")
Set fso = CreateObject("Scripting.FileSystemObject")

scriptDir = fso.GetParentFolderName(WScript.ScriptFullName)
command = "powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File """ & scriptDir & "\launch-kaizen-max.ps1"""

shell.Run command, 0, False
