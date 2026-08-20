# Applies the bundled udev rule to devices that are already plugged in.
#
# Without this, a board connected before the install stays root:dialout until
# it is unplugged and reconnected -- which reads, from the app, exactly like a
# board that is not there. Both commands are tolerated failing: an image build
# or a container has no running udevd, and that is not an install error.
if command -v udevadm > /dev/null 2>&1; then
    udevadm control --reload-rules > /dev/null 2>&1 || :
    udevadm trigger --subsystem-match=tty > /dev/null 2>&1 || :
fi
exit 0
