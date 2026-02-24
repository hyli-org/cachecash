#!/bin/sh

cd /usr/share/nginx/html

envsubst < index.html > index.html.tmp && mv index.html.tmp index.html

exec "$@"
