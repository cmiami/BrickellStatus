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
				set position of every item to {760, 100}
			end tell

			tell the icon view options of container window
				set icon size to 128
				set text size to 16
				set arrangement to not arranged
				set background picture to file ".background:dmg-background.png"
			end tell

			set position of item appName to {180, 210}
			set the extension hidden of item appName to true
			set position of item "Applications" to {480, 210}

			update without registering applications
			delay 1
			close container window
		end tell
	end tell

	delay 2
end run
