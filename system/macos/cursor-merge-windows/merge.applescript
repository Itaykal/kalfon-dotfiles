-- Cursor: click Window > Merge All Windows.
-- Retries for a few seconds because the menu may not be fully populated right after launch.

on run
	delay 2

	tell application "System Events"
		if not (exists process "Cursor") then return

		tell process "Cursor"
			repeat 10 times
				try
					set winMenu to menu "Window" of menu bar 1
					if exists menu item "Merge All Windows" of winMenu then
						set mergeItem to menu item "Merge All Windows" of winMenu
						if enabled of mergeItem then
							click mergeItem
							return
						end if
					end if
				end try
				delay 1
			end repeat
		end tell
	end tell
end run
