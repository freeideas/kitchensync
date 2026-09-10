# KitchenSync

KitchenSync is for people who keep the same files in more than one place: photos on a laptop and a cloud drive, project archives on a workstation and a USB drive, a movie library on two drives in two houses. It is one small program. No daemon, no account, no config file, no tray icon, no "sign in to continue". If you can reach a folder by path or by SSH, it is a peer. That is the whole onboarding.

We wrote it in Rust because that is the most fashionable programming language right now. Some unintended bonuses: It is fast, tiny, unable to corrupt its own memory, and works the same on Windows, macOS, and Linux.

## A story you may recognize

You have a folder of music files. It lives on a cloud drive, on your MacBook, and on the Windows PC in the other room.

Over the summer you added a few dozen new albums on the Windows PC. On the MacBook, you finally deleted the ones you are embarrassed about. On the cloud drive, you renamed half the folders to fix the capitalization, because it was bothering you.

Oh crap. Now nothing matches anything.

If syncthing had been configured correctly and running in the background EVERY GODDAMN MINUTE ON ALL THREE DEVICES, you would be OK. But now syncing these devices will make this mess even messier.

If you had been using KitchenSync once in a while, this would have been no problemo. Each change would have gone out to the other two peers the next time you ran it, which is what you wanted.

That is fine. You can start using KitchenSync now, and it is still no problemo:

```
kitchensync c:/music ~/Music sftp://you@cloud/music
```

After a little cleanup and running it again, everything will be synced, and it will stay synced, as long as you run kitchensync once in a while. Go ahead and add, delete, and rename things anywhere you want; kitchensync will apply the changes to all of your devices. When two devices disagree about a file, the newest change wins.

## Why not just use rsync (etc.) ?

We did. For years.

| feature                       | KitchenSync | rsync         | Syncthing     | Unison     | robocopy            |
| ----------------------------- | ----------- | ------------- | ------------- | ---------- | ------------------- |
| Tells you what it is doing    | Immediately | After an hour | On a web page | Eventually | A cryptic wall      |
| Syncs both ways               | Yes         | No            | Yes           | Yes        | No                  |
| Copies several files at once  | Yes         | No            | Yes           | No         | Maybe with `/MT`    |
| Peers per run                 | Many        | 2             | Many          | 2          | 2                   |
| Requires background processes | No          | No            | ON EVERY PEER | Optional   | No                  |
| PhDs to configure             | 0           | 0.5           | 2             | 1          | 0.5, with caps lock |
| Undo a bad run                | `--undo`    | No            | Sort of       | No         | No                  |
| Overwritten files kept        | 90 days     | If you asked  | Sometimes     | No         | No                  |
| Requires installed remotely   | SSH         | SSH + rsync   | Syncthing     | SSH+Unison | Windows or Samba    |
| Windows, macOS, Linux         | All three   | Good luck     | All three     | Good luck  | Windows only.       |
| Dry run                       | Yes         | Yes           | No            | Yes        | Yes                 |
| Ignore like .gitignore        | Yes         | With a spell  | Almost        | No         | No                  |
| Written in                    | Rust        | prehistoric C | Go            | OCaml      | No one knows        |
| One peer, several addresses   | Yes         | No            | With Relays   | No         | No                  |

Pain points, itemized:

- **rsync** goes silent for an hour (even with dry-run!), copies one block, then goes silent again. One file at a time, like it is 1996, because that's when it was written. Deleted files are gone unless you passed `--backup`, which you did not.
- **Syncthing** needs a daemon on every machine. Configuring it means folder IDs, device IDs, introducers, relays, and a maybe even a phone app to scan a QR code. Versioning exists, if you turned it on, per folder, per device, before the accident. MIT should teach post-grad classes on how to configure it.
- **Unison** needs to be installed on both ends, at the exact same version, and has a profile grammar you can learn but will forget. There is no undo.
- **Robocopy** Windows only. It has about 80 flags, all in uppercase. We will concede that its dry-run is good; Windows only (worth saying again).

All of these are fine, respectable tools that we have used for many years and that have hurt our souls in specific ways. Corrections welcome.

## Getting KitchenSync

Ready-to-run executables are in `released/`: `kitchensync.exe` for Windows, `kitchensync.mac` for macOS, and `kitchensync.linux` for Linux. Copy the one for your platform somewhere on your `PATH` and rename it to `kitchensync` if you like. There is nothing to install. On macOS and Linux you may need `chmod +x` after copying.

## How to run

```
kitchensync [options] <peer> <peer> [<peer>...]
```

Run `kitchensync` with no arguments (or with `--help`, `-h`, `/?`, or whatever) for the full list of options. All progress and diagnostic output goes to stdout, from the very first second.

## First Sync

If one peer already holds the copy you trust, mark it with `+`. That peer is the canon: it wins every disagreement, and the others are made to match it, including deleting what it does not have. After first sync, you won't need the `+`.

```
kitchensync +c:/photos sftp://bilbo@cloud/volume1/photos
```

Forgot the `+` on first sync? No problem; now every peer has every file; maybe that's what you wanted anyway.

## Everything Else

**More peers.** Add them to the command. A new peer receives the group's current state before it gets a vote. Mark a peer with `-` to make it subordinate for one run: it is overwritten to match the group and never argues back.

```
kitchensync c:/photos sftp://bilbo@cloud/volume1/photos d:/backup/photos -/mnt/usb/photos
```

**Ignore things.** `-x` takes the same patterns as a `.gitignore`, `-x @file` reads them from a file, and a `.kitchensync/ignore` in a peer's root applies every time. `.DS_Store`, `._*`, `Thumbs.db`, and `desktop.ini` are ignored out of the box.

```
kitchensync c:/appz d:/appz -x "PortablePlatform/PortableApps" -x "*.tmp"
```

**Undo.** The first line of every run is the command that puts things back. `--undo` with the same peers rolls each one back to just before its newest run. Displaced files are kept 90 days (`--keep-bak-days`).

```
undo later with: kitchensync --rollback 2026-09-10_15-48-41_413346Z c:/photos sftp://bilbo@cloud/volume1/photos
```

**One peer, several addresses.** Put the ways to reach it in brackets; the first that answers is used. Connection tuning can ride along on each URL.

```
kitchensync c:/photos "[h:/office-share/photos,sftp://192.168.1.50:2222/photos?timeout-conn=20,sftp://cloud.vpn/photos?timeout-conn=60]"
```

## For developers and other geeks

Detailed behavior is defined in the documents under `specs/`. Start with `specs/DEVELOPMENT.md`, which explains how the project is laid out, built, and tested, and indexes the other specs.
