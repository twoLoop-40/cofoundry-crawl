use std::collections::VecDeque;

/// BFS URL frontier (CrawlerPipelineSpec §1: URL queue)
pub struct Frontier {
    queue: VecDeque<(String, usize)>, // (url, depth)
}

impl Frontier {
    pub fn new() -> Self {
        Self {
            queue: VecDeque::new(),
        }
    }

    pub fn push(&mut self, url: String, depth: usize) {
        self.queue.push_back((url, depth));
    }

    pub fn pop(&mut self) -> Option<(String, usize)> {
        self.queue.pop_front()
    }

    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.queue.len()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bfs_order() {
        let mut f = Frontier::new();
        f.push("http://a.com".into(), 0);
        f.push("http://b.com".into(), 1);
        f.push("http://c.com".into(), 1);

        assert_eq!(f.len(), 3);
        assert_eq!(f.pop().unwrap().0, "http://a.com");
        assert_eq!(f.pop().unwrap().0, "http://b.com");
        assert_eq!(f.pop().unwrap().0, "http://c.com");
        assert!(f.is_empty());
    }
}
