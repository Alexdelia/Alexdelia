limit=300
status=0
cargo_integration_test='^(crate/[^/]+/)?tests/'

for path in "$@"; do
	if [[ $path == *_test.rs || $path =~ $cargo_integration_test ]]; then
		continue
	fi
	line_count=$(wc -l <"$path")
	if ((line_count > limit)); then
		echo "$path: $line_count lines (limit $limit)"
		status=1
	fi
done

exit "$status"
