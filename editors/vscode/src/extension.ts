import * as fs from 'fs';
import * as vscode from 'vscode';
import {
    LanguageClient,
    LanguageClientOptions,
    ServerOptions,
    State,
    TransportKind,
} from 'vscode-languageclient/node';

import { ensureServerBinary, UnsupportedPlatformError } from './downloader';

const CLIENT_ID = 'gdscript-lsp';
const CLIENT_NAME = 'GDScript LSP';

let client: LanguageClient | undefined;
let statusBar: vscode.StatusBarItem;
let outputChannel: vscode.OutputChannel;

export async function activate(context: vscode.ExtensionContext): Promise<void> {
    outputChannel = vscode.window.createOutputChannel(CLIENT_NAME);
    context.subscriptions.push(outputChannel);

    statusBar = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 0);
    statusBar.command = 'gdscript-lsp.restart';
    statusBar.show();
    context.subscriptions.push(statusBar);

    context.subscriptions.push(
        vscode.commands.registerCommand('gdscript-lsp.restart', async () => {
            await stopClient();
            await startClient(context);
        }),
    );

    await startClient(context);
}

export async function deactivate(): Promise<void> {
    await stopClient();
}

async function startClient(context: vscode.ExtensionContext): Promise<void> {
    setStatus('starting');

    let serverPath: string;
    try {
        serverPath = await resolveServerPath(context);
    } catch (err) {
        setStatus('error');
        const msg = err instanceof Error ? err.message : String(err);
        outputChannel.appendLine(`Failed to resolve server binary: ${msg}`);
        const action = await vscode.window.showErrorMessage(
            `GDScript LSP: failed to obtain server binary — ${msg}`,
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
        return;
    }

    const gdformatPath = vscode.workspace.getConfiguration(CLIENT_ID).get<string>('gdformatPath');

    const serverOptions: ServerOptions = {
        command: serverPath,
        transport: TransportKind.stdio,
        options: gdformatPath ? { env: { ...process.env, GDFORMAT_PATH: gdformatPath } } : undefined,
    };

    const clientOptions: LanguageClientOptions = {
        documentSelector: [{ scheme: 'file', pattern: '**/*.gd' }],
        synchronize: {
            fileEvents: vscode.workspace.createFileSystemWatcher('**/*.gd'),
        },
        outputChannel,
        initializationOptions: gdformatPath ? { gdformatPath } : undefined,
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
        outputChannel.appendLine(`Failed to start server: ${msg}`);
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

async function resolveServerPath(context: vscode.ExtensionContext): Promise<string> {
    const config = vscode.workspace.getConfiguration(CLIENT_ID);
    const configured = config.get<string>('serverPath');
    if (configured) {
        if (!fs.existsSync(configured)) {
            throw new Error(`gdscript-lsp.serverPath is set to "${configured}", but no file exists there.`);
        }
        return configured;
    }

    try {
        return await ensureServerBinary(context, outputChannel);
    } catch (err) {
        if (err instanceof UnsupportedPlatformError) {
            throw err;
        }
        throw new Error(
            `Could not download the gdscript-lsp binary (${(err as Error).message}). ` +
                'Set gdscript-lsp.serverPath to point at a manually installed binary.',
        );
    }
}
