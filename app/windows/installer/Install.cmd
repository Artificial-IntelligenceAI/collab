@echo off
rem Double-click this. It only exists so the script beside it can be run without
rem anybody having to know about PowerShell execution policies.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0Install.ps1"
