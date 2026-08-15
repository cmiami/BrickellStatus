-- Positions the two items in the installer window.
--
-- The background picture is cosmetic and is the one step that reliably fails on
-- a headless builder (-10006). It is therefore attempted separately and its
-- failure ignored: losing the artwork is a blemish, while losing the positions
-- leaves the app and the Applications alias stacked on top of each other, which
-- makes the drag-and-drop the window exists for impossible.
--
-- Nothing is parked off-screen either. A parking step that succeeds while a
-- later positioning step fails is exactly how two items end up in one place.

on run arguments
	set volumeName to item 1 of arguments
	set appName to item 2 of arguments

	tell application "Finder"
		tell disk volumeName
			open
			delay 1

			tell container window
				set current view to icon view
				set toolbar visible to false
				set statusbar visible to false
				set the bounds to {10, 60, 670, 460}
			end tell

			tell the icon view options of container window
				set icon size to 128
				set text size to 16
				set arrangement to not arranged
			end tell

			-- Artwork only. A builder with no window server refuses this.
			try
				tell the icon view options of container window
					set background picture to file ".background:dmg-background.png"
				end tell
			end try

			-- The part that matters: side by side, drag from left to right.
			set position of item appName to {180, 210}
			set position of item "Applications" to {480, 210}
			try
				set the extension hidden of item appName to true
			end try

			update without registering applications
			delay 1
			close container window
		end tell
	end tell

	delay 2
end run
