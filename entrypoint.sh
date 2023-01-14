DATABASE_URL="postgresql://$POSTGRES_USER:$POSTGRES_PASSWORD@db/$POSTGRES_DB
/bobot/sea-orm-cli migrate up && /bobot/bobot --config /config/config.toml
