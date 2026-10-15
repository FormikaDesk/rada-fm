# Phones, cameras and sticks

Linux. The **DEVICES** part of the sidebar lists, besides the mounted disks:

| What | Shown when | What rada runs |
|---|---|---|
| a **phone** (MTP) or a **camera** (PTP) the desktop has opened | it is a folder of the virtual filesystem (`$XDG_RUNTIME_DIR/gvfs/mtp:host=…`) | nothing to list it; `gio mount -u` to eject it |
| a phone or a camera that is **plugged in and not mounted** | `gio mount -li` lists it | `gio mount <uri>` (**Mount**) |
| a **USB stick or card** that is plugged in and not mounted | `lsblk` lists a removable drive with a filesystem rada can open (not an encrypted or RAID member) and no mount point | `udisksctl mount -b <device>` (**Mount**) |
| a stick mounted **anywhere**, `/mnt/stick` included | the drive is flagged removable or sits on the USB bus (read from `/sys`) | `udisksctl unmount` and `power-off` (**Eject**) |

An entry that is not mounted says so after its name (*Canon EOS  not mounted*); `Enter`, a click or its menu mounts it, and once it is open it is an ordinary entry. The programs run with the C locale and a time limit, in a thread of their own, so a phone that does not answer never holds the list of disks. Nothing is mounted or unmounted unless you ask.

**What to install.** On Arch: `gvfs-mtp` for phones (it brings `libmtp`), `gvfs-gphoto2` for cameras, `udisks2` for sticks; `gvfs` is the base. Without a tool, **Mount** says what is missing (*"this system cannot open phones: install gvfs-mtp (on Arch: pacman -S gvfs-mtp)"*) and rada goes on. A phone must be unlocked and set to *File transfer* in its USB notification; if another program holds it (a photo app, another file manager) the message says so.

**Copying to and from a phone** is the same plan, progress bar and cancel as any copy, off the interface thread. The plan tells you, before anything is done, what is different there:

- the data reaches the phone when the file is **closed**, so a big file can wait at 100% for a while; each file is written under its final name (no temporary file, no rename), and one the phone did not keep whole is removed and reported;
- **permissions, owner and extended attributes are not kept**, dates only if the phone accepts them; what comes from a phone gets the usual permissions (`644`, `755`);
- **links** cannot be stored on a phone and are skipped (said); files that are hard links of each other become separate files;
- renaming and moving are **not atomic** and some phones refuse them; a move between two phones or two storages of one phone is a copy, verified, then the source is removed;
- a phone has **no trash**: deleting copies each item to your trash first (slow for big files), checks it and then removes it from the phone;
- **undo is not guaranteed** for anything that puts something back on a phone (the plan says so; undo of a copy *from* a phone just removes the copy);
- an **archive** cannot be written straight onto a phone: make it in a folder here, then copy it.

If a phone does not show up, `rada debug devices` prints what rada sees and why each thing is listed or left out (read only; nothing is mounted).

A move and a verified copy read the file back from the phone before the source is touched. Nothing is counted or read in the background on a phone: no folder sizes, no tag reads, no recursive scans, and the preview of a file there says it is not read (the phone may send the whole file). A phone listing is as quick as the phone is.
