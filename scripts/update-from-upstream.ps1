param(
    [string]$UpstreamRemote = "upstream",
    [string]$OriginRemote = "origin",
    [string]$MainBranch = "main",
    [string]$DevBranch = "",
    [switch]$NoPause
)

$ErrorActionPreference = "Stop"

function Write-Step {
    param([string]$Message)
    Write-Host ""
    Write-Host "==> $Message" -ForegroundColor Cyan
}

function Write-Warn {
    param([string]$Message)
    Write-Host "!!  $Message" -ForegroundColor Yellow
}

function Invoke-Git {
    param([Parameter(ValueFromRemainingArguments = $true)][string[]]$Args)

    & git @Args
    if ($LASTEXITCODE -ne 0) {
        throw "git $($Args -join ' ') failed with exit code $LASTEXITCODE"
    }
}

function Pause-BeforeExit {
    if (-not $NoPause) {
        Write-Host ""
        Read-Host "Press Enter to exit"
    }
}

try {
    Write-Step "Checking Git repository"
    Invoke-Git rev-parse --is-inside-work-tree | Out-Null
    $repoRoot = (& git rev-parse --show-toplevel).Trim()
    if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($repoRoot)) {
        throw "Could not find the repository root"
    }
    Set-Location $repoRoot
    Write-Host "Repository: $repoRoot"

    $currentBranch = (& git branch --show-current).Trim()
    if ([string]::IsNullOrWhiteSpace($currentBranch)) {
        throw "Detached HEAD is not supported for safety"
    }
    if ([string]::IsNullOrWhiteSpace($DevBranch)) {
        $DevBranch = $currentBranch
    }
    if ($DevBranch -eq $MainBranch) {
        throw "The development branch cannot be $MainBranch. Switch to your development branch first."
    }

    Write-Step "Checking for local changes"
    $dirty = (& git status --porcelain)
    if ($dirty) {
        Write-Warn "Uncommitted or unstaged changes were found."
        Write-Host $dirty
        throw "Commit or stash local changes before running this script."
    }

    Write-Step "Checking remotes and branches"
    Invoke-Git remote get-url $UpstreamRemote | Out-Null
    Invoke-Git remote get-url $OriginRemote | Out-Null
    Invoke-Git show-ref --verify --quiet "refs/heads/$MainBranch"
    Invoke-Git show-ref --verify --quiet "refs/heads/$DevBranch"

    Write-Step "Fetching latest remote code"
    Invoke-Git fetch --prune $UpstreamRemote
    Invoke-Git fetch --prune $OriginRemote
    Invoke-Git show-ref --verify --quiet "refs/remotes/$UpstreamRemote/$MainBranch"

    $timestamp = Get-Date -Format "yyyyMMdd-HHmmss"
    $backupBranch = "backup/$DevBranch-before-upstream-$timestamp"

    Write-Step "Creating safety backup branch: $backupBranch"
    Invoke-Git branch $backupBranch $DevBranch

    Write-Step "Fast-forwarding local $MainBranch to $UpstreamRemote/$MainBranch"
    Invoke-Git switch $MainBranch
    try {
        Invoke-Git merge --ff-only "$UpstreamRemote/$MainBranch"
    }
    catch {
        Invoke-Git switch $DevBranch
        throw "Local $MainBranch cannot fast-forward to $UpstreamRemote/$MainBranch. Inspect $MainBranch manually. Backup branch: $backupBranch"
    }

    Write-Step "Merging latest $MainBranch into $DevBranch"
    Invoke-Git switch $DevBranch
    try {
        Invoke-Git merge $MainBranch --no-edit
    }
    catch {
        Write-Warn "Merge failed or produced conflicts. Git is left in the conflict state for manual resolution."
        Write-Host "Backup branch: $backupBranch"
        Write-Host "After resolving conflicts: git add <files> ; git commit"
        Write-Host "To abandon this merge: git merge --abort"
        throw
    }

    Write-Step "Done"
    Write-Host "Development branch $DevBranch now includes the latest $MainBranch."
    Write-Host "Safety backup branch: $backupBranch"
    Write-Host "This script did not push. Push manually after reviewing the result."
}
catch {
    Write-Host ""
    Write-Host "Failed: $($_.Exception.Message)" -ForegroundColor Red
    exit 1
}
finally {
    Pause-BeforeExit
}
