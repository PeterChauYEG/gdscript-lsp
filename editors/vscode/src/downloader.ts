import * as crypto from 'crypto';
import * as fs from 'fs';
import * as https from 'https';
import * as os from 'os';
import * as path from 'path';
import * as vscode from 'vscode';

const REPO_OWNER = 'PeterChauYEG';
const REPO_NAME = 'gdscript-lsp';
const USER_AGENT = 'gdscript-lsp-vscode-extension';
const BINARY_NAME = process.platform === 'win32' ? 'gdscript-lsp.exe' : 'gdscript-lsp';
const VERSION_FILE = '.version';

interface GitHubAsset {
    name: string;
    browser_download_url: string;
}

interface GitHubRelease {
    tag_name: string;
    assets: GitHubAsset[];
}

// Maps a platform/arch pair to the asset name published on GitHub Releases.
// The macOS x86_64 name matches the existing release pipeline's artifact naming.
const PLATFORM_ASSET_NAMES: Record<string, string> = {
    'linux-x64': 'gdscript-lsp-linux-x86_64',
    'linux-arm64': 'gdscript-lsp-linux-arm64',
    'darwin-x64': 'gdscript-lsp-macos-x86_64',
    'darwin-arm64': 'gdscript-lsp-macos-arm64',
    'win32-x64': 'gdscript-lsp-windows-x86_64.exe',
};

export class UnsupportedPlatformError extends Error {}

function platformKey(): string {
    const arch = os.arch() === 'arm64' ? 'arm64' : 'x64';
    return `${process.platform}-${arch}`;
}

function assetNameForPlatform(): string {
    const key = platformKey();
    const name = PLATFORM_ASSET_NAMES[key];
    if (!name) {
        throw new UnsupportedPlatformError(
            `No gdscript-lsp release binary is published for platform "${key}". ` +
                `Supported platforms: ${Object.keys(PLATFORM_ASSET_NAMES).join(', ')}`,
        );
    }
    return name;
}

function binDir(context: vscode.ExtensionContext): string {
    return path.join(context.extensionPath, 'bin');
}

function binaryPath(context: vscode.ExtensionContext): string {
    return path.join(binDir(context), BINARY_NAME);
}

function versionFilePath(context: vscode.ExtensionContext): string {
    return path.join(binDir(context), VERSION_FILE);
}

function httpsGetJson<T>(url: string): Promise<T> {
    return new Promise((resolve, reject) => {
        https.get(
            url,
            { headers: { 'User-Agent': USER_AGENT, Accept: 'application/vnd.github+json' } },
            (res) => {
                if (res.statusCode && res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
                    httpsGetJson<T>(res.headers.location).then(resolve, reject);
                    return;
                }
                if (res.statusCode !== 200) {
                    reject(new Error(`GitHub API request failed: HTTP ${res.statusCode}`));
                    res.resume();
                    return;
                }
                const chunks: Buffer[] = [];
                res.on('data', (chunk) => chunks.push(chunk));
                res.on('end', () => {
                    try {
                        resolve(JSON.parse(Buffer.concat(chunks).toString('utf8')) as T);
                    } catch (err) {
                        reject(err);
                    }
                });
            },
        ).on('error', reject);
    });
}

function httpsGetText(url: string): Promise<string> {
    return new Promise((resolve, reject) => {
        https.get(url, { headers: { 'User-Agent': USER_AGENT } }, (res) => {
            if (res.statusCode && res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
                httpsGetText(res.headers.location).then(resolve, reject);
                return;
            }
            if (res.statusCode !== 200) {
                reject(new Error(`Download failed: HTTP ${res.statusCode} for ${url}`));
                res.resume();
                return;
            }
            const chunks: Buffer[] = [];
            res.on('data', (chunk) => chunks.push(chunk));
            res.on('end', () => resolve(Buffer.concat(chunks).toString('utf8')));
        }).on('error', reject);
    });
}

function httpsDownloadToFile(url: string, destPath: string): Promise<void> {
    return new Promise((resolve, reject) => {
        https.get(url, { headers: { 'User-Agent': USER_AGENT } }, (res) => {
            if (res.statusCode && res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
                httpsDownloadToFile(res.headers.location, destPath).then(resolve, reject);
                return;
            }
            if (res.statusCode !== 200) {
                reject(new Error(`Download failed: HTTP ${res.statusCode} for ${url}`));
                res.resume();
                return;
            }
            const file = fs.createWriteStream(destPath);
            res.pipe(file);
            file.on('finish', () => file.close(() => resolve()));
            file.on('error', reject);
        }).on('error', reject);
    });
}

function sha256File(filePath: string): Promise<string> {
    return new Promise((resolve, reject) => {
        const hash = crypto.createHash('sha256');
        const stream = fs.createReadStream(filePath);
        stream.on('data', (chunk) => hash.update(chunk));
        stream.on('end', () => resolve(hash.digest('hex')));
        stream.on('error', reject);
    });
}

function parseChecksum(checksumFileContents: string): string {
    const token = checksumFileContents.trim().split(/\s+/)[0];
    if (!token) {
        throw new Error('Checksum file was empty');
    }
    return token.toLowerCase();
}

async function fetchLatestRelease(): Promise<GitHubRelease> {
    const url = `https://api.github.com/repos/${REPO_OWNER}/${REPO_NAME}/releases/latest`;
    return httpsGetJson<GitHubRelease>(url);
}

async function downloadAndVerify(
    release: GitHubRelease,
    context: vscode.ExtensionContext,
    log: vscode.OutputChannel,
): Promise<void> {
    const assetName = assetNameForPlatform();
    const asset = release.assets.find((a) => a.name === assetName);
    if (!asset) {
        throw new Error(
            `Release ${release.tag_name} does not include an asset named "${assetName}". ` +
                'Set gdscript-lsp.serverPath to use a manually installed binary instead.',
        );
    }
    const checksumAsset = release.assets.find((a) => a.name === `${assetName}.sha256`);

    fs.mkdirSync(binDir(context), { recursive: true });
    const destPath = binaryPath(context);
    const tmpPath = `${destPath}.download`;

    log.appendLine(`Downloading ${assetName} from ${release.tag_name}…`);
    await httpsDownloadToFile(asset.browser_download_url, tmpPath);

    if (checksumAsset) {
        log.appendLine('Verifying checksum…');
        const expected = parseChecksum(await httpsGetText(checksumAsset.browser_download_url));
        const actual = await sha256File(tmpPath);
        if (expected !== actual) {
            fs.rmSync(tmpPath, { force: true });
            throw new Error(
                `Checksum mismatch for ${assetName}: expected ${expected}, got ${actual}. Download aborted.`,
            );
        }
        log.appendLine('Checksum OK.');
    } else {
        log.appendLine(`WARNING: no ${assetName}.sha256 published for ${release.tag_name}; skipping verification.`);
    }

    fs.renameSync(tmpPath, destPath);
    if (process.platform !== 'win32') {
        fs.chmodSync(destPath, 0o755);
    }
    fs.writeFileSync(versionFilePath(context), release.tag_name, 'utf8');
    log.appendLine(`Installed gdscript-lsp ${release.tag_name} to ${destPath}`);
}

function currentInstalledVersion(context: vscode.ExtensionContext): string | undefined {
    try {
        return fs.readFileSync(versionFilePath(context), 'utf8').trim();
    } catch {
        return undefined;
    }
}

/**
 * Ensures a gdscript-lsp binary is present and up to date, downloading it from the
 * latest GitHub Release when missing or outdated. Returns the path to the binary.
 */
export async function ensureServerBinary(
    context: vscode.ExtensionContext,
    log: vscode.OutputChannel,
): Promise<string> {
    const destPath = binaryPath(context);
    const installedVersion = currentInstalledVersion(context);

    let release: GitHubRelease;
    try {
        release = await fetchLatestRelease();
    } catch (err) {
        if (installedVersion && fs.existsSync(destPath)) {
            log.appendLine(
                `WARNING: could not check for updates (${(err as Error).message}); using cached ${installedVersion}.`,
            );
            return destPath;
        }
        throw err;
    }

    if (installedVersion === release.tag_name && fs.existsSync(destPath)) {
        log.appendLine(`gdscript-lsp ${installedVersion} is up to date.`);
        return destPath;
    }

    await vscode.window.withProgress(
        {
            location: vscode.ProgressLocation.Notification,
            title: `Downloading GDScript LSP server ${release.tag_name}…`,
        },
        () => downloadAndVerify(release, context, log),
    );

    return destPath;
}
