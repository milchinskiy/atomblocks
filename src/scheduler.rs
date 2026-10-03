use std::time::{Duration, Instant};

#[derive(Debug)]
struct State {
    interval: Option<Duration>,
    next_deadline: Option<Instant>,
    running: bool,
    pending_manual: bool,
}

#[derive(Debug)]
pub(crate) struct Scheduler {
    states: Vec<State>,
}

impl Scheduler {
    pub fn new(intervals: impl IntoIterator<Item = Option<Duration>>, now: Instant) -> Self {
        let states = intervals
            .into_iter()
            .map(|interval| State {
                next_deadline: interval.map(|_| now),
                interval,
                running: false,
                pending_manual: false,
            })
            .collect();
        Self { states }
    }

    pub fn due(&mut self, now: Instant) -> Vec<usize> {
        let mut ready = Vec::new();

        for (index, state) in self.states.iter_mut().enumerate() {
            let (Some(interval), Some(deadline)) = (state.interval, state.next_deadline) else {
                continue;
            };
            if deadline > now {
                continue;
            }

            state.next_deadline = Some(advance_deadline(deadline, interval, now));
            if !state.running {
                state.running = true;
                ready.push(index);
            }
        }

        ready
    }

    pub fn hit(&mut self, index: usize) -> Option<usize> {
        let state = self.states.get_mut(index)?;
        if state.running {
            state.pending_manual = true;
            None
        } else {
            state.running = true;
            Some(index)
        }
    }

    pub fn complete(&mut self, index: usize) -> Option<usize> {
        let state = self.states.get_mut(index)?;
        state.running = false;
        if state.pending_manual {
            state.pending_manual = false;
            state.running = true;
            Some(index)
        } else {
            None
        }
    }

    pub fn next_deadline(&self) -> Option<Instant> {
        self.states
            .iter()
            .filter_map(|state| state.next_deadline)
            .min()
    }
}

fn advance_deadline(deadline: Instant, interval: Duration, now: Instant) -> Instant {
    let elapsed = now.saturating_duration_since(deadline);
    let interval_nanos = interval.as_nanos();
    debug_assert!(interval_nanos > 0);
    let steps = elapsed.as_nanos() / interval_nanos + 1;

    multiply_duration(interval, steps)
        .and_then(|delta| deadline.checked_add(delta))
        .or_else(|| now.checked_add(interval))
        .unwrap_or(now)
}

fn multiply_duration(duration: Duration, factor: u128) -> Option<Duration> {
    let nanos = duration.as_nanos().checked_mul(factor)?;
    let seconds = nanos / 1_000_000_000;
    if seconds > u128::from(u64::MAX) {
        return None;
    }
    Some(Duration::new(
        seconds as u64,
        (nanos % 1_000_000_000) as u32,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positive_intervals_run_immediately_and_missed_ticks_are_skipped() {
        let now = Instant::now();
        let period = Duration::from_millis(100);
        let mut scheduler = Scheduler::new([Some(period)], now);

        assert_eq!(scheduler.due(now), vec![0]);
        assert!(scheduler.due(now + Duration::from_secs(1)).is_empty());
        assert_eq!(scheduler.complete(0), None);
        assert!(scheduler.due(now + Duration::from_millis(1_001)).is_empty());
        assert_eq!(scheduler.due(now + Duration::from_millis(1_100)), vec![0]);
    }

    #[test]
    fn manual_hits_coalesce_to_one_follow_up_run() {
        let now = Instant::now();
        let mut scheduler = Scheduler::new([None], now);

        assert_eq!(scheduler.hit(0), Some(0));
        assert_eq!(scheduler.hit(0), None);
        assert_eq!(scheduler.hit(0), None);
        assert_eq!(scheduler.complete(0), Some(0));
        assert_eq!(scheduler.complete(0), None);
    }

    #[test]
    fn manual_only_blocks_have_no_deadline() {
        let scheduler = Scheduler::new([None, None], Instant::now());
        assert!(scheduler.next_deadline().is_none());
    }
}
