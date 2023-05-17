DATABASE_URL="postgresql://$POSTGRES_USER:$POSTGRES_PASSWORD@db/$POSTGRES_DB
/bobot/sea-orm-cli migrate up && /bobot/dijkstra --config /config/config.toml
