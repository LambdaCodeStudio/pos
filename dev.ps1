# Utilidades de desarrollo. Uso: .\dev.ps1 <comando>
#   up       levanta TODO como procesos independientes (Postgres + API + app en :8080)
#   down     detiene la API y la app (Postgres queda corriendo)
#   db-start | db-stop | db-psql | run | test | check
param([Parameter(Mandatory = $true)][string]$Comando)

$raiz = $PSScriptRoot
$pg = "$raiz\.dev\pgsql\bin"
$pgdata = "$raiz\.dev\pgdata"
$mingw = "$env:USERPROFILE\mingw64-portable\mingw64\bin"
$env:Path = "$env:USERPROFILE\.cargo\bin;$mingw;$env:Path"

switch ($Comando) {
    'up' {
        & "$pg\pg_ctl.exe" -D $pgdata status 2>&1 | Out-Null
        if ($LASTEXITCODE -ne 0) { & "$pg\pg_ctl.exe" -D $pgdata -l "$raiz\.dev\pg.log" -o "-p 5432" start }
        if (-not (Test-Path "$raiz\target\debug\pos.exe")) { cargo build }
        Start-Process -FilePath "$raiz\target\debug\pos.exe" -WorkingDirectory $raiz -WindowStyle Hidden
        Start-Process -FilePath (Get-Command node).Source -ArgumentList 'servidor.mjs' -WorkingDirectory "$raiz\frontend" -WindowStyle Hidden
        Write-Host "POS levantado: app en http://localhost:8080 (API :3000, Postgres :5432)"
    }
    'down' {
        Stop-Process -Name pos -Force -ErrorAction SilentlyContinue
        Get-CimInstance Win32_Process -Filter "Name = 'node.exe'" |
            Where-Object { $_.CommandLine -like '*servidor.mjs*' } |
            ForEach-Object { Stop-Process -Id $_.ProcessId -Force }
        Write-Host "API y app detenidas (Postgres sigue corriendo; .\dev.ps1 db-stop para pararlo)"
    }
    'db-start' { & "$pg\pg_ctl.exe" -D $pgdata -l "$raiz\.dev\pg.log" -o "-p 5432" start }
    'db-stop'  { & "$pg\pg_ctl.exe" -D $pgdata stop }
    'db-psql'  { & "$pg\psql.exe" -U postgres -h localhost -p 5432 -d pos }
    'run'      { cargo run }
    'test'     { cargo test }
    'check'    { cargo check --all-targets }
    default    { Write-Host "Comando desconocido: $Comando" }
}
