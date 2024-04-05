awk < ./CHANGELOG.md '
	/^## Unreleased/ {}
	/^## [0-9]/ {
		if (++section == 2) {
			exit
		}
	}
	section
'

