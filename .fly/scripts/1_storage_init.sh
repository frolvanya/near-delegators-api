FOLDER=/data
if [ ! -d "$FOLDER" ]; then
    echo "$FOLDER is not a directory, copying data_ content to data"
    cp -r /data_/. /data
    echo "deleting data_..."
    rm -rf /data_
fi
