# `actix_diesel_cache`

[![Docs](https://docs.rs/actix_diesel_cache/badge.svg)](https://docs.rs/crate/actix_diesel_cache/)
[![Crates.io](https://img.shields.io/crates/v/actix_diesel_cache.svg)](https://crates.io/crates/actix_diesel_cache)

A library with actor which provides caching for small and rarely changing tables in databases.

## Usage

Add to `Cargo.toml`:

```toml
actix_diesel_cache = "0.1.0"
```

## Example

```rust
use diesel::prelude::*;

table! {
    shop (id) {
        id -> Int4,
        name -> Text,
        address -> Text,
    }
}

#[derive(Queryable, Insertable, Clone, Debug, Eq, PartialEq)]
#[table_name = "shop"]
struct Shop {
    id: i32,
    name: String,
    address: String,
}

impl actix_diesel_cache::Cache<SqliteConnection, shop::table> for Shop {
    type Id = i32;
    fn get_id(&self) -> Self::Id {
        s.id
    }
}

async fn example(conn: SqliteConnection) -> actix_diesel_cache::Result<()> {
    let addr = actix_diesel_cache::CacheDbActor::new(conn)?.start();

    let shop = Shop {
        id: 1,
        name: "Adidas",
        address: "Central street",
    };
    addr.send(actix_diesel_cache::Save(shop)).await.unwrap()?;
    let shop1 = addr.send(actix_diesel_cache::Get(shop.id)).await.unwrap()?;;

    assert_eq!(shop, shop1);

    let shops = addr.send(actix_diesel_cache::GetAll::default()).await.unwrap()?;;

    assert_eq!(shops, vec![shop]);
}
```
