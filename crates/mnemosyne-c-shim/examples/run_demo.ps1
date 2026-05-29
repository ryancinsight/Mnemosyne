# run_demo.ps1 — Compiles and runs the interposition demo on Windows.
#

$ShimDir = Split-Path -Parent $MyInvocation.MyCommand.Definition
$WorkspaceDir = Resolve-Path "$ShimDir\..\..\.."

Write-Host "=== Building mnemosyne-c-shim cdylib ===" -ForegroundColor Cyan
Push-Location $ShimDir\..
cargo build --release
Pop-Location

# Locate gcc from MSYS2
$GccPath = "D:\msys64\ucrt64\bin\gcc.exe"
if (!(Test-Path $GccPath)) {
    $GccCmd = Get-Command gcc -ErrorAction SilentlyContinue
    if ($GccCmd) {
        $GccPath = $GccCmd.Source
    }
}

if (Test-Path $GccPath) {
    Write-Host "=== Compiling C interpose_demo using GCC ===" -ForegroundColor Cyan
    # Run compilation inside MSYS2 bash environment to ensure all paths are correct
    $BashPath = "D:\msys64\usr\bin\bash.exe"
    if (Test-Path $BashPath) {
        & $BashPath -c "export PATH=/ucrt64/bin:/usr/bin:`$PATH; gcc -O2 -Icrates/mnemosyne-c-shim/include -o target/release/interpose_demo_gcc.exe crates/mnemosyne-c-shim/examples/interpose_demo.c -Ltarget/release -lmnemosyne_c_shim"
    } else {
        & $GccPath -O2 -Iinclude -o "$ShimDir\..\..\..\target\release\interpose_demo_gcc.exe" "$ShimDir\interpose_demo.c" -L"$ShimDir\..\..\..\target\release" -lmnemosyne_c_shim
    }

    if (Test-Path "$WorkspaceDir\target\release\interpose_demo_gcc.exe") {
        Write-Host "=== Running compiled interpose_demo ===" -ForegroundColor Cyan
        if (Test-Path $BashPath) {
            & $BashPath -c "target/release/interpose_demo_gcc.exe"
        } else {
            Push-Location "$WorkspaceDir\target\release"
            .\interpose_demo_gcc.exe
            Pop-Location
        }
    } else {
        Write-Error "Compilation failed. interpose_demo_gcc.exe was not created."
    }
} else {
    Write-Warning "MinGW/MSYS2 gcc.exe was not found. Attempting MSVC compile..."
    # Fallback to MSVC if cl is in PATH
    $ClCmd = Get-Command cl -ErrorAction SilentlyContinue
    if ($ClCmd) {
        Write-Host "=== Compiling C interpose_demo using MSVC cl ===" -ForegroundColor Cyan
        Push-Location "$WorkspaceDir\target\release"
        cl /I"$ShimDir\include" "$ShimDir\examples\interpose_demo.c" /link mnemosyne_c_shim.dll.lib /out:interpose_demo_msvc.exe
        if (Test-Path "interpose_demo_msvc.exe") {
            Write-Host "=== Running compiled interpose_demo (MSVC) ===" -ForegroundColor Cyan
            .\interpose_demo_msvc.exe
        } else {
            Write-Error "MSVC Compilation failed."
        }
        Pop-Location
    } else {
        Write-Error "No suitable C compiler (gcc or cl) was found. Please ensure MinGW/MSYS2 or Visual Studio Build Tools are installed and in PATH."
    }
}
