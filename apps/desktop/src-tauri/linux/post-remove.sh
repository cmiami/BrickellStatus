# Drops the udev rule this package installed from the running udevd, so a
# removed app stops holding open ACLs on serial devices it no longer drives.
if command -v udevadm > /dev/null 2>&1; then
    udevadm control --reload-rules > /dev/null 2>&1 || :
fi
exit 0
