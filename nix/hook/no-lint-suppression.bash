if rg --line-number --with-filename '#!?\[(allow|expect)\(|cfg_attr\(.*(allow|expect)\b|coverage\(off\)|cfg\(not\(coverage|cfg_attr\(coverage' "$@"; then
	exit 1
fi
