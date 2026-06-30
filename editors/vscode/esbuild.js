const esbuild = require('esbuild');

const production = process.argv.includes('--production');
const watch = process.argv.includes('--watch');

const ctx = esbuild.context({
  entryPoints: ['src/extension.ts'],
  bundle: true,
  outfile: 'out/extension.js',
  external: ['vscode'],
  format: 'cjs',
  platform: 'node',
  target: 'node18',
  sourcemap: !production,
  minify: production,
  logLevel: 'info',
});

ctx.then(async (c) => {
  if (watch) {
    await c.watch();
    console.log('Watching for changes…');
  } else {
    await c.rebuild();
    await c.dispose();
  }
}).catch(() => process.exit(1));
