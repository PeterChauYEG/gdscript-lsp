import * as fs from 'fs';
import * as path from 'path';
import * as vscode from 'vscode';
import {
    LanguageClient,
    LanguageClientOptions,
    ServerOptions,
    State,
    TransportKind,
} from 'vscode-languageclient/node';

const CLIENT_ID = 'gdscript-lsp';
const CLIENT_NAME = 'GDScript LSP';

let client: LanguageClient | undefined;
let statusBar: vscode.StatusBarItem;

export async function activate(context: vscode.ExtensionContext): Promise<void> {
    statusBar = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 0);
    statusBar.command = 'gdscript-lsp.restart';
    statusBar.show();
    context.subscriptions.push(statusBar);

    context.subscriptions.push(
        vscode.commands.registerCommand('gdscript-lsp.restart', async () => {
            await stopClient();
            await startClient();
        }),
    );

    await startClient();
}

export async function deactivate(): Promise<void> {
    await stopClient();
}

async function startClient(): Promise<void> {
    const serverPath = resolveServerPath();
    setStatus('starting');

    const serverOptions: ServerOptions = {
        command: serverPath,
        transport: TransportKind.stdio,
    };

    const clientOptions: LanguageClientOptions = {
        documentSelector: [{ scheme: 'file', pattern: '**/*.gd' }],
        synchronize: {
            fileEvents: vscode.workspace.createFileSystemWatcher('**/*.gd'),
        },
    };

    client = new LanguageClient(CLIENT_ID, CLIENT_NAME, serverOptions, clientOptions);

    client.onDidChangeState(({ newState }) => {
        if (newState === State.Running) {
            setStatus('running', serverPath);
        } else if (newState === State.Stopped) {
            setStatus('stopped');
        } else {
            setStatus('starting');
        }
    });

    try {
        await client.start();
    } catch (err) {
        setStatus('error');
        const msg = err instanceof Error ? err.message : String(err);
        const action = await vscode.window.showErrorMessage(
            `GDScript LSP: failed to start — ${msg}`,
            'Set Server Path',
            'View Releases',
        );
        if (action === 'Set Server Path') {
            await vscode.commands.executeCommand(
                'workbench.action.openSettings',
                'gdscript-lsp.serverPath',
            );
        } else if (action === 'View Releases') {
            await vscode.env.openExternal(
                vscode.Uri.parse('https://github.com/PeterChauYEG/gdscript-lsp/releases'),
            );
        }
    }
}

async function stopClient(): Promise<void> {
    if (client) {
        await client.stop();
        client = undefined;
    }
}

function setStatus(state: 'starting' | 'running' | 'stopped' | 'error', detail?: string): void {
    switch (state) {
        case 'starting':
            statusBar.text = '$(loading~spin) GDScript';
            statusBar.tooltip = `${CLIENT_NAME}: starting…`;
            statusBar.backgroundColor = undefined;
            break;
        case 'running':
            statusBar.text = '$(check) GDScript';
            statusBar.tooltip = `${CLIENT_NAME}: running${detail ? ` (${detail})` : ''}\nClick to restart`;
            statusBar.backgroundColor = undefined;
            break;
        case 'stopped':
            statusBar.text = '$(circle-slash) GDScript';
            statusBar.tooltip = `${CLIENT_NAME}: stopped\nClick to restart`;
            statusBar.backgroundColor = new vscode.ThemeColor('statusBarItem.warningBackground');
            break;
        case 'error':
            statusBar.text = '$(error) GDScript';
            statusBar.tooltip = `${CLIENT_NAME}: failed to start\nClick to retry`;
            statusBar.backgroundColor = new vscode.ThemeColor('statusBarItem.errorBackground');
            break;
    }
}

function resolveServerPath(): string {
    const config = vscode.workspace.getConfiguration(CLIENT_ID);
    const configured = config.get<string>('serverPath');
    if (configured) {
        return configured;
    }

    const home = process.env.HOME ?? process.env.USERPROFILE ?? '';
    const ext = process.platform === 'win32' ? '.exe' : '';
    const binary = `gdscript-lsp${ext}`;

    const candidates: string[] = [
        path.join(home, '.local', 'bin', binary),
        `/opt/homebrew/bin/${binary}`,
        `/usr/local/bin/${binary}`,
    ];

    for (const candidate of candidates) {
        if (fs.existsSync(candidate)) {
            return candidate;
        }
    }

    // Fall through to PATH — if not found, the client will surface the error.
    return binary;
}
