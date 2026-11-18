//! [Advertiser]: keeps one [ServiceRecord] announced through a [Responder] while it lives.

use std::net::IpAddr;

use crate::{Error, InstanceName, ServiceRecord};

/// Something that announces records on the LAN: the in-process mDNS responder or Avahi.
///
/// Calls block for a short, bounded time (a message to the responder's thread or a D-Bus round trip), so an async
/// caller may make them from its task.
pub trait Responder: Send {
    /// Announces `record`, replacing an earlier announcement of the same instance.
    fn publish(&mut self, record: &ServiceRecord) -> Result<(), Error>;

    /// Withdraws the instance: browsers see it go (a goodbye with TTL 0).
    fn withdraw(&mut self, instance: &InstanceName) -> Result<(), Error>;
}

impl<R: Responder + ?Sized> Responder for Box<R> {
    fn publish(&mut self, record: &ServiceRecord) -> Result<(), Error> {
        (**self).publish(record)
    }

    fn withdraw(&mut self, instance: &InstanceName) -> Result<(), Error> {
        (**self).withdraw(instance)
    }
}

/// Keeps one server announced: published while it has addresses, updated when they change, withdrawn on drop.
pub struct Advertiser<R: Responder = Box<dyn Responder>> {
    responder: R,
    record: ServiceRecord,
    published: bool,
}

impl<R: Responder> Advertiser<R> {
    /// Announces `record` through `responder`, unless it has no addresses yet.
    pub fn new(mut responder: R, record: ServiceRecord) -> Result<Self, Error> {
        let published = !record.addresses().is_empty();
        if published {
            responder.publish(&record)?;
        }
        Ok(Self {
            responder,
            record,
            published,
        })
    }

    /// The record as announced.
    pub fn record(&self) -> &ServiceRecord {
        &self.record
    }

    /// Whether the record is on the LAN now: it has addresses and was not withdrawn.
    pub fn is_published(&self) -> bool {
        self.published
    }

    /// Announces new addresses, after an interface came up, went down or got a new lease. The same set in another
    /// order changes nothing; no addresses withdraw the record until some come back. Returns whether anything
    /// was sent.
    pub fn set_addresses(
        &mut self,
        addresses: impl IntoIterator<Item = IpAddr>,
    ) -> Result<bool, Error> {
        let mut next = self.record.clone();
        next.set_addresses(addresses)?;
        let mut old: Vec<IpAddr> = self.record.addresses().to_vec();
        let mut new: Vec<IpAddr> = next.addresses().to_vec();
        old.sort_unstable();
        new.sort_unstable();
        if old == new {
            return Ok(false);
        }
        self.record = next;
        if self.record.addresses().is_empty() {
            self.responder.withdraw(self.record.instance())?;
            self.published = false;
        } else {
            self.responder.publish(&self.record)?;
            self.published = true;
        }
        Ok(true)
    }

    /// Withdraws the record now and reports a failure, which dropping only logs.
    pub fn withdraw(mut self) -> Result<(), Error> {
        self.withdraw_now()
    }

    fn withdraw_now(&mut self) -> Result<(), Error> {
        if !self.published {
            return Ok(());
        }
        self.published = false;
        self.responder.withdraw(self.record.instance())
    }
}

impl<R: Responder> Drop for Advertiser<R> {
    fn drop(&mut self) {
        if let Err(e) = self.withdraw_now() {
            tracing::warn!(instance = %self.record.instance(), "withdrawing the DNS-SD record failed: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU16;
    use std::sync::{Arc, Mutex};

    use ww_version::{FullVersion, Version, WireVersion};

    use super::*;
    use crate::{InstanceId, Txt};

    #[derive(Debug, PartialEq)]
    enum Call {
        Publish(Vec<IpAddr>),
        Withdraw(String),
    }

    #[derive(Clone, Default)]
    struct Fake(Arc<Mutex<Vec<Call>>>);

    impl Fake {
        fn take(&self) -> Vec<Call> {
            std::mem::take(&mut *self.0.lock().unwrap())
        }
    }

    impl Responder for Fake {
        fn publish(&mut self, record: &ServiceRecord) -> Result<(), Error> {
            self.0
                .lock()
                .unwrap()
                .push(Call::Publish(record.addresses().to_vec()));
            Ok(())
        }

        fn withdraw(&mut self, instance: &InstanceName) -> Result<(), Error> {
            self.0
                .lock()
                .unwrap()
                .push(Call::Withdraw(instance.as_str().to_owned()));
            Ok(())
        }
    }

    fn record() -> ServiceRecord {
        let txt = Txt::new(
            "/ww",
            &FullVersion::new("tv_api", Version::new(0, 1, 0)),
            Some(WireVersion::new(1, 0)),
            InstanceId::new("tv").unwrap(),
        )
        .unwrap();
        ServiceRecord::new(
            InstanceName::new("TV").unwrap(),
            NonZeroU16::new(7590).unwrap(),
            txt,
        )
    }

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn appears_changes_and_goes() {
        let fake = Fake::default();
        let r = record().with_addresses([ip("10.0.0.2")]).unwrap();
        let mut adv = Advertiser::new(fake.clone(), r).unwrap();
        assert_eq!(fake.take(), [Call::Publish(vec![ip("10.0.0.2")])]);

        assert!(adv.set_addresses([ip("10.0.0.3")]).unwrap());
        assert_eq!(fake.take(), [Call::Publish(vec![ip("10.0.0.3")])]);

        drop(adv);
        assert_eq!(fake.take(), [Call::Withdraw("TV".to_owned())]);
    }

    #[test]
    fn same_set_in_another_order_sends_nothing() {
        let fake = Fake::default();
        let r = record()
            .with_addresses([ip("10.0.0.2"), ip("fd00::2")])
            .unwrap();
        let mut adv = Advertiser::new(fake.clone(), r).unwrap();
        fake.take();
        assert!(!adv.set_addresses([ip("fd00::2"), ip("10.0.0.2")]).unwrap());
        assert_eq!(fake.take(), []);
    }

    #[test]
    fn no_addresses_withdraw_until_they_come_back() {
        let fake = Fake::default();
        let mut adv = Advertiser::new(fake.clone(), record()).unwrap();
        assert!(!adv.is_published());
        assert_eq!(fake.take(), []);

        adv.set_addresses([ip("10.0.0.2")]).unwrap();
        assert!(adv.is_published());
        adv.set_addresses([]).unwrap();
        assert!(!adv.is_published());
        assert_eq!(
            fake.take(),
            [
                Call::Publish(vec![ip("10.0.0.2")]),
                Call::Withdraw("TV".to_owned())
            ]
        );

        // nothing left to withdraw on drop
        drop(adv);
        assert_eq!(fake.take(), []);
    }

    #[test]
    fn bad_addresses_change_nothing() {
        let fake = Fake::default();
        let r = record().with_addresses([ip("10.0.0.2")]).unwrap();
        let mut adv = Advertiser::new(fake.clone(), r).unwrap();
        fake.take();
        assert!(adv.set_addresses([ip("224.0.0.251")]).is_err());
        assert_eq!(adv.record().addresses(), [ip("10.0.0.2")]);
        assert_eq!(fake.take(), []);
    }
}
