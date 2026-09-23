#!/usr/bin/env node

/**
 * Build a release, package what it produced, and leave the tree small again.
 *
 * The three steps are one command because they are one intention: a release
 * nobody can install is not finished, and the debug cache a build leaves behind
 * is tens of gigabytes that no installer needs. The package lands outside the
 * repository — a directory that gets cleaned must not be where the deliverable
 * lives.
 */

import { spawnSync } from 'node:child_process';
import {
    copyFileSync,
    existsSync,
    mkdirSync,
    readdirSync,
    readFileSync,
    rmSync,
    statSync,
    writeFileSync,
} from 'node:fs';
import { createHash } from 'node:crypto';
import path from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const TARGET_DIR = path.join(REPO_ROOT, 'src-tauri', 'target');
const BUNDLE_DIR = path.join(TARGET_DIR, 'release', 'bundle');
const PRODUCT_NAME = 'TauriTavern';

/** What an installer looks like, by extension, on any platform this builds on. */
const INSTALLER_EXTENSIONS = ['.msi', '.exe', '.dmg', '.deb', '.rpm', '.appimage', '.pkg', '.apk'];

/** Caches a build leaves behind that nothing needs once the release is out. */
const DEBUG_SCRATCH = ['debug', 'incremental-stale-moved'];

/**
 * Files that live in the target root because a one-off investigation put them
 * there. Kept off the list are the ones cargo itself reads (`.rustc_info.json`,
 * `CACHEDIR.TAG`) and the tool archives a rebuild would otherwise re-download.
 */
const TARGET_ROOT_KEEP = new Set(['.rustc_info.json', 'CACHEDIR.TAG']);
const TARGET_ROOT_KEEP_PREFIX = ['nasm-'];

function printHelp() {
    console.log(`Usage: node scripts/build-release-package.mjs [options]

Builds a release, packages its installers, and clears the debug build cache.

Options:
  --skip-build         Package what is already in target/release/bundle
  --keep-debug         Do not clear the debug build cache
  --output-dir <path>  Where the package goes (default: <repo>/../${PRODUCT_NAME}-release)
  --no-zip             Copy the installers out without zipping them
  --help               Show this help message
`);
}

function parseArgs(argv) {
    const options = {
        skipBuild: false,
        keepDebug: false,
        outputDir: path.resolve(REPO_ROOT, '..', `${PRODUCT_NAME}-release`),
        zip: true,
    };

    for (let index = 0; index < argv.length; index += 1) {
        const value = argv[index];
        if (value === '--help' || value === '-h') {
            printHelp();
            process.exit(0);
        }
        if (value === '--skip-build') {
            options.skipBuild = true;
            continue;
        }
        if (value === '--keep-debug') {
            options.keepDebug = true;
            continue;
        }
        if (value === '--no-zip') {
            options.zip = false;
            continue;
        }
        if (value === '--output-dir') {
            const outputDir = argv[index + 1];
            if (!outputDir) {
                throw new Error('Missing value for --output-dir');
            }
            options.outputDir = path.resolve(outputDir);
            index += 1;
            continue;
        }
        throw new Error(`Unknown option: ${value}`);
    }

    return options;
}

function run(command, args) {
    const result = spawnSync(command, args, { cwd: REPO_ROOT, stdio: 'inherit' });
    if (result.error) {
        throw result.error;
    }
    if (result.status !== 0) {
        process.exit(result.status ?? 1);
    }
}

function formatBytes(bytes) {
    const gigabytes = bytes / (1024 ** 3);
    if (gigabytes >= 1) {
        return `${gigabytes.toFixed(2)} GB`;
    }
    return `${(bytes / (1024 ** 2)).toFixed(1)} MB`;
}

/** Every file under a directory, pairing its path with its size. */
function walkFiles(directory) {
    const found = [];
    for (const entry of readdirSync(directory, { recursive: true, withFileTypes: true })) {
        if (!entry.isFile()) {
            continue;
        }
        // `parentPath` is the current name for what older runtimes call `path`.
        const parent = entry.parentPath ?? entry.path ?? directory;
        const absolute = path.join(parent, entry.name);
        found.push({ path: absolute, size: statSync(absolute).size });
    }
    return found;
}

function directorySize(directory) {
    if (!existsSync(directory)) {
        return 0;
    }
    return walkFiles(directory).reduce((total, file) => total + file.size, 0);
}

/**
 * The installers the build produced, newest first.
 *
 * Named by extension rather than by a list of bundle kinds: the same release
 * builds an `.msi` on Windows, a `.dmg` on macOS and a `.deb` on Debian, and
 * the packaging step has no business knowing which one it is looking at.
 */
function collectInstallers() {
    if (!existsSync(BUNDLE_DIR)) {
        return [];
    }

    return walkFiles(BUNDLE_DIR)
        .filter((file) => INSTALLER_EXTENSIONS.includes(path.extname(file.path).toLowerCase()))
        .map((file) => ({ ...file, name: path.basename(file.path) }))
        .sort((left, right) => statSync(right.path).mtimeMs - statSync(left.path).mtimeMs);
}

function sha256(filePath) {
    const hash = createHash('sha256');
    hash.update(readFileSync(filePath));
    return hash.digest('hex');
}

function copyOut(installers, outputDir) {
    mkdirSync(outputDir, { recursive: true });
    const copied = [];
    for (const installer of installers) {
        const destination = path.join(outputDir, installer.name);
        copyFileSync(installer.path, destination);
        copied.push({ ...installer, path: destination });
    }

    const sums = copied
        .map((file) => `${sha256(file.path)}  ${file.name}`)
        .join('\n');
    writeFileSync(path.join(outputDir, 'SHA256SUMS.txt'), `${sums}\n`, 'utf8');

    return copied;
}

/**
 * Zip the package with whatever this platform already has.
 *
 * The installers are copied out before this runs, so a machine without a zip
 * tool still gets the thing it has to have — and is told plainly what is
 * missing rather than being handed a half-written archive.
 */
function zipPackage(outputDir, fileNames) {
    const archive = path.join(outputDir, `${PRODUCT_NAME}-${releaseTag()}.zip`);
    rmSync(archive, { force: true });

    const isWindows = process.platform === 'win32';
    const command = isWindows ? 'powershell.exe' : 'zip';
    const args = isWindows
        ? [
            '-NoProfile',
            '-NonInteractive',
            '-Command',
            `Compress-Archive -LiteralPath ${fileNames.map((name) => `'${name}'`).join(',')} -DestinationPath '${path.basename(archive)}' -Force`,
        ]
        : ['-q', '-j', path.basename(archive), ...fileNames];

    const result = spawnSync(command, args, { cwd: outputDir, stdio: 'inherit' });
    if (result.error || result.status !== 0) {
        console.warn(`Could not create ${path.basename(archive)}; the installers are already in ${outputDir}.`);
        return null;
    }
    return archive;
}

/** The version the package is named after, read from the app's own manifest. */
function releaseTag() {
    const manifest = path.join(REPO_ROOT, 'package.json');
    const { version } = JSON.parse(readFileSync(manifest, 'utf8'));
    return `${version}-${process.platform === 'win32' ? 'windows' : process.platform}-${process.arch}`;
}

/**
 * Delete a path, refusing anything outside the build's own target directory.
 *
 * The guard is the point: this function runs unattended at the end of a build,
 * and an argument that resolved somewhere unexpected would be deleting somebody's
 * work. Fail fast instead.
 */
function removeInsideTarget(relative) {
    const absolute = path.resolve(TARGET_DIR, relative);
    const root = path.resolve(TARGET_DIR) + path.sep;
    if (!absolute.startsWith(root)) {
        throw new Error(`Refusing to delete outside the target directory: ${absolute}`);
    }
    if (!existsSync(absolute)) {
        return 0;
    }

    const size = statSync(absolute).isDirectory() ? directorySize(absolute) : statSync(absolute).size;
    rmSync(absolute, { recursive: true, force: true, maxRetries: 3, retryDelay: 200 });
    return size;
}

/** The debug cache and the scratch files, which no installer needs. */
function clearDebugBuild() {
    let reclaimed = 0;
    for (const scratch of DEBUG_SCRATCH) {
        reclaimed += removeInsideTarget(scratch);
    }

    for (const entry of readdirSync(TARGET_DIR, { withFileTypes: true })) {
        if (entry.isDirectory() || TARGET_ROOT_KEEP.has(entry.name)) {
            continue;
        }
        if (TARGET_ROOT_KEEP_PREFIX.some((prefix) => entry.name.startsWith(prefix))) {
            continue;
        }
        reclaimed += removeInsideTarget(entry.name);
    }

    return reclaimed;
}

function main() {
    const options = parseArgs(process.argv.slice(2));

    if (!options.skipBuild) {
        console.log('Building the release...');
        run(process.execPath, [path.join('scripts', 'tauri-app.mjs'), '--prepare-frontend', 'build']);
    }

    const installers = collectInstallers();
    if (installers.length === 0) {
        throw new Error(
            `No installers in ${BUNDLE_DIR}. Run the build first, or drop --skip-build.`,
        );
    }

    const copied = copyOut(installers, options.outputDir);
    const archive = options.zip
        ? zipPackage(options.outputDir, [...copied.map((file) => file.name), 'SHA256SUMS.txt'])
        : null;

    console.log(`Packaged ${copied.length} installer(s) into ${options.outputDir}:`);
    for (const file of copied) {
        console.log(`  ${file.name}  ${formatBytes(file.size)}`);
    }
    if (archive) {
        console.log(`  ${path.basename(archive)}  ${formatBytes(statSync(archive).size)}`);
    }

    // Said before the cleanup, not after: the package is the point of the run,
    // and a cache that refuses to go away must not be what the last word is about.
    console.log('');
    console.log('To install: run the "-setup.exe" (it asks whether to install for every user');
    console.log('or only for you, and lets the directory be chosen). The ".msi" is the one to');
    console.log('hand to a deployment tool, where nobody is there to answer a wizard.');

    if (!options.keepDebug) {
        try {
            const reclaimed = clearDebugBuild();
            console.log('');
            console.log(`Cleared the debug build cache, reclaiming ${formatBytes(reclaimed)}.`);
        } catch (error) {
            throw new Error(
                `The package is built and copied, but the debug build cache could not be cleared: ${error instanceof Error ? error.message : String(error)}`,
            );
        }
    }
}

try {
    main();
} catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exit(1);
}
