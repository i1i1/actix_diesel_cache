//! actix_diesel_cache is crate which provides the actix actor for caching all
//! database entries on local machine.

#![deny(missing_docs)]

use std::collections::HashMap;
use std::fmt::Debug;
use std::hash::Hash;
use std::marker::{PhantomData, Unpin};
use std::sync::Arc;

use actix::prelude::*;

use diesel::associations::HasTable;
use diesel::connection::Connection;
use diesel::deserialize::Queryable;
use diesel::insertable::CanInsertInSingleQuery;
use diesel::prelude::*;
use diesel::query_builder::{AsQuery, QueryFragment, QueryId};
use diesel::sql_types::HasSqlType;

/// Error of cache actor
pub type Error = diesel::result::Error;

/// Result
pub type Result<V> = std::result::Result<V, Error>;

/// Trait for CacheDbActor. Requires at compile time for type to be queryable in
/// table and database backend.
///
/// Connection backend should have all types in table.
pub trait Cache<Conn, Table>:
    Queryable<Table::SqlType, Conn::Backend> + Sized + Debug + Clone + 'static
where
    Conn: Connection + Unpin + 'static,
    Conn::Backend: HasSqlType<Table::SqlType>,
    Table: diesel::Table + HasTable<Table = Table> + AsQuery,
    Table::Query: QueryId + QueryFragment<Conn::Backend>,
{
    /// Id type for getting specific records
    type Id: Hash + Eq + Clone;

    /// Get id of item
    fn get_id(&self) -> Self::Id;

    /// Read all entries from db
    fn read_all(c: &Conn) -> Result<HashMap<Self::Id, Self>> {
        let vec: Vec<Self> = Table::table().load(c)?;
        let mut out = HashMap::with_capacity(vec.len());
        for it in vec {
            let id = it.get_id();
            out.insert(id, it);
        }
        Ok(out)
    }

    /// Write one entry to db.
    ///
    /// Entry type should be insertable in table and its sqltype should be
    /// insertable in one query.
    fn write_one<W>(w: W, c: &Conn) -> Result<usize>
    where
        Table::FromClause: QueryFragment<Conn::Backend>,
        W: Insertable<Table>,
        W::Values: CanInsertInSingleQuery<Conn::Backend>
            + QueryFragment<Conn::Backend>,
    {
        diesel::insert_into(Table::table()).values(w).execute(c)
    }
}

/// Actix Actor for caching database.
/// Has fast reads and slow writes. Updates its records once in a minute and on inserts.
pub struct CacheDbActor<Conn, Table, C>
where
    Conn: Connection + Unpin + 'static,
    Conn::Backend: HasSqlType<Table::SqlType>,
    Table: diesel::Table + HasTable<Table = Table> + AsQuery,
    Table::Query: QueryId + QueryFragment<Conn::Backend>,
    C: Cache<Conn, Table>,
{
    /// Connection for db
    conn: Conn,
    /// All items read from db
    cache: Arc<HashMap<C::Id, C>>,
    /// Phantom marker for saving table inside structure
    t: PhantomData<Table>,
}

/// Save one entry
#[derive(Debug)]
pub struct Save<T>(pub T);

impl<T: 'static> actix::Message for Save<T> {
    type Result = Result<()>;
}

/// Gets item by id
#[derive(Debug)]
pub struct Get<Conn, Table, C>
where
    Conn: Connection + Unpin + 'static,
    Conn::Backend: HasSqlType<Table::SqlType>,
    Table: diesel::Table + HasTable<Table = Table> + AsQuery,
    Table::Query: QueryId + QueryFragment<Conn::Backend>,
    C: Cache<Conn, Table>,
{
    /// Id of item to get
    pub id: C::Id,
}

impl<Conn, Table, C> Clone for Get<Conn, Table, C>
where
    Conn: Connection + Unpin + 'static,
    Conn::Backend: HasSqlType<Table::SqlType>,
    Table: diesel::Table + HasTable<Table = Table> + AsQuery,
    Table::Query: QueryId + QueryFragment<Conn::Backend>,
    C: Cache<Conn, Table>,
    C::Id: Clone,
{
    fn clone(&self) -> Self {
        Self {
            id: self.id.clone(),
        }
    }
}

impl<Conn, Table, C> Copy for Get<Conn, Table, C>
where
    Conn: Connection + Unpin + 'static,
    Conn::Backend: HasSqlType<Table::SqlType>,
    Table: diesel::Table + HasTable<Table = Table> + AsQuery,
    Table::Query: QueryId + QueryFragment<Conn::Backend>,
    C: Cache<Conn, Table>,
    C::Id: Clone + Copy,
{
}

impl<Conn, Table, C> actix::Message for Get<Conn, Table, C>
where
    Conn: Connection + Unpin + 'static,
    Conn::Backend: HasSqlType<Table::SqlType>,
    Table: diesel::Table + HasTable<Table = Table> + AsQuery,
    Table::Query: QueryId + QueryFragment<Conn::Backend>,
    C: Cache<Conn, Table>,
{
    type Result = Result<Option<C>>;
}

/// Gets all entries
#[derive(Debug, Clone, Copy)]
pub struct GetAll<Conn, Table, C>
where
    Conn: Connection + Unpin + 'static,
    Conn::Backend: HasSqlType<Table::SqlType>,
    Table: diesel::Table + HasTable<Table = Table> + AsQuery + Unpin + 'static,
    Table::Query: QueryId + QueryFragment<Conn::Backend>,
    C: Cache<Conn, Table> + 'static,
{
    _c: PhantomData<(Conn, Table, C)>,
}

impl<Conn, Table, C> Default for GetAll<Conn, Table, C>
where
    Conn: Connection + Unpin + 'static,
    Conn::Backend: HasSqlType<Table::SqlType>,
    Table: diesel::Table + HasTable<Table = Table> + AsQuery + Unpin + 'static,
    Table::Query: QueryId + QueryFragment<Conn::Backend>,
    C: Cache<Conn, Table> + 'static,
{
    fn default() -> Self {
        GetAll {
            _c: Default::default(),
        }
    }
}

impl<Conn, Table, C> actix::Message for GetAll<Conn, Table, C>
where
    Conn: Connection + Unpin + 'static,
    Conn::Backend: HasSqlType<Table::SqlType>,
    Table: diesel::Table + HasTable<Table = Table> + AsQuery + Unpin + 'static,
    Table::Query: QueryId + QueryFragment<Conn::Backend>,
    C: Cache<Conn, Table> + 'static,
{
    type Result = Result<Arc<HashMap<C::Id, C>>>;
}

impl<Conn, Table, C> CacheDbActor<Conn, Table, C>
where
    Conn: Connection + Unpin + 'static,
    Conn::Backend: HasSqlType<Table::SqlType>,
    Table: diesel::Table + HasTable<Table = Table> + AsQuery + Unpin + 'static,
    Table::Query: QueryId + QueryFragment<Conn::Backend>,
    C: Cache<Conn, Table>,
{
    /// Constructor
    pub fn new(conn: Conn) -> Result<Self> {
        let (cache, t) = Default::default();
        let mut s = Self { conn, cache, t };
        s.update()?;
        Ok(s)
    }

    fn update(&mut self) -> Result<()> {
        self.cache = Arc::new(C::read_all(&self.conn)?);
        Ok(())
    }

    fn get(&self, id: C::Id) -> Option<C> {
        self.cache.get(&id).cloned()
    }

    fn timer_update(&mut self, context: &mut Context<Self>) {
        let dur = std::time::Duration::from_secs(60);
        let _ = self.update();
        TimerFunc::new(dur, Self::timer_update).spawn(context);
    }
}

impl<Conn, Table, C> Actor for CacheDbActor<Conn, Table, C>
where
    Conn: Connection + Unpin + 'static,
    Conn::Backend: HasSqlType<Table::SqlType>,
    Table: diesel::Table + HasTable<Table = Table> + AsQuery + Unpin + 'static,
    Table::Query: QueryId + QueryFragment<Conn::Backend>,
    C: Cache<Conn, Table>,
{
    type Context = Context<Self>;

    fn started(&mut self, context: &mut Context<Self>) {
        self.timer_update(context)
    }
}

impl<Conn, Table, C> Handler<GetAll<Conn, Table, C>>
    for CacheDbActor<Conn, Table, C>
where
    Conn: Connection + Unpin + 'static,
    Conn::Backend: HasSqlType<Table::SqlType>,
    Table: diesel::Table + HasTable<Table = Table> + AsQuery + Unpin + 'static,
    Table::Query: QueryId + QueryFragment<Conn::Backend>,
    C: Cache<Conn, Table>,
{
    type Result = Result<Arc<HashMap<C::Id, C>>>;

    fn handle(
        &mut self,
        _: GetAll<Conn, Table, C>,
        _: &mut Context<Self>,
    ) -> Self::Result {
        // Flushing not by timer because we are not supposed to have error in
        // exported data.
        self.update()?;
        Ok(Arc::clone(&self.cache))
    }
}

impl<Conn, Table, C, W> Handler<Save<W>> for CacheDbActor<Conn, Table, C>
where
    Conn: Connection + Unpin + 'static,
    Conn::Backend: HasSqlType<Table::SqlType>,
    Table: diesel::Table + HasTable<Table = Table> + AsQuery + Unpin + 'static,
    Table::Query: QueryId + QueryFragment<Conn::Backend>,
    Table::FromClause: QueryFragment<Conn::Backend>,
    C: Cache<Conn, Table>,
    W: Insertable<Table> + 'static,
    W::Values:
        CanInsertInSingleQuery<Conn::Backend> + QueryFragment<Conn::Backend>,
{
    type Result = Result<()>;

    fn handle(&mut self, pred: Save<W>, _: &mut Context<Self>) -> Self::Result {
        C::write_one(pred.0, &self.conn)?;
        self.update()?;
        Ok(())
    }
}

impl<Conn, Table, C> Handler<Get<Conn, Table, C>>
    for CacheDbActor<Conn, Table, C>
where
    Conn: Connection + Unpin + 'static,
    Conn::Backend: HasSqlType<Table::SqlType>,
    Table: diesel::Table + HasTable<Table = Table> + AsQuery + Unpin + 'static,
    Table::Query: QueryId + QueryFragment<Conn::Backend>,
    C: Cache<Conn, Table>,
{
    type Result = Result<Option<C>>;

    fn handle(
        &mut self,
        Get { id }: Get<Conn, Table, C>,
        _: &mut Context<Self>,
    ) -> Self::Result {
        match self.get(id.clone()) {
            Some(out) => Ok(Some(out)),
            None => {
                self.update()?;
                Ok(self.get(id))
            }
        }
    }
}
