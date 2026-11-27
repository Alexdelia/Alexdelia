limit=300
status=0

for path in "$@"; do
	case "$path" in
	*_test.rs | tests/* | */tests/*) continue ;;
	esac
	line_count=$(wc -l <"$path")
	if ((line_count > limit)); then
		echo "$path: $line_count lines (limit $limit)"
		status=1
	fi
done

exit "$status"
