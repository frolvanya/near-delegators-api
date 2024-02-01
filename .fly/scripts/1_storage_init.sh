FOLDER=/
if [ ! -d "$FOLDER" ]; then
    echo "$FOLDER is not a directory, copying storage_ content to storage"
    cp -r /_. /
    echo "deleting storage_..."
    rm -rf /_
fi
