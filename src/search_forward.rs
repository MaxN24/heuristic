use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, RwLock,
    },
    thread,
    time::Instant,
};

use hashbrown::HashMap;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};

use crate::{
    backwards_poset::BackwardsPoset,
    bitset::BitSet,
    cache::Cache,
    constants::{LOWER_BOUNDS, UPPER_BOUNDS},
    free_poset::FreePoset,
    poset::Poset,
    pseudo_canonified_poset::PseudoCanonifiedPoset,
    search_backward::start_search_backward,
    utils::format_duration,
    WeightFunction,
};

pub struct Search<'a> {
    n: u8,
    i: u8,
    current_max: u8,
    cache: &'a mut Cache,
    analytics: Analytics,
    comparisons: &'a mut HashMap<PseudoCanonifiedPoset, (u8, u8)>,
    use_bidirectional_search: bool,
    weight_function: WeightFunction,
    heuristic_strategy: u8,
}

#[derive(Debug, Clone, Copy)]
pub enum Cost {
    /// Not solved. Impossible in less than the number of comparisons
    Minimum(u8),
    /// Solved in the number of comparisons
    Solved(u8),
}

pub struct Analytics {
    total_posets: u64,
    cache_hits: u64,
    cache_misses: u64,
    cache_replaced: u64,
    max_progress_depth: u8,
    multiprogress: MultiProgress,
    progress_bars: Vec<(ProgressBar, AtomicU64)>,
}

impl Cost {
    pub fn value(self) -> u8 {
        match self {
            Cost::Minimum(min) => min,
            Cost::Solved(solved) => solved,
        }
    }

    pub fn is_solved(self) -> bool {
        matches!(self, Cost::Solved(_))
    }
}

impl<'a> Search<'a> {
    pub fn new(
        n: u8,
        i: u8,
        cache: &'a mut Cache,
        comparisons: &'a mut HashMap<PseudoCanonifiedPoset, (u8, u8)>,
        use_bidirectional_search: bool,
        weight_function: WeightFunction,
        heuristic_strategy: u8,
    ) -> Self {
        Search {
            n,
            i,
            current_max: 0,
            cache,
            analytics: Analytics::new(n.max(4) - 3),
            comparisons,
            use_bidirectional_search,
            weight_function,
            heuristic_strategy,
        }
    }

    fn search_cache(&mut self, poset: &PseudoCanonifiedPoset) -> Option<Cost> {
        let result = self.cache.get_mut(poset);
        if result.is_some() {
            self.analytics.record_hit();
        } else {
            self.analytics.record_miss();
        }
        result
    }

    #[inline]
    fn peek_cache(&self, poset: &PseudoCanonifiedPoset) -> Option<Cost> {
        self.cache.get(poset)
    }

    fn insert_cache(&mut self, poset: PseudoCanonifiedPoset, new_cost: Cost) {
        if let Some(cost) = self.cache.get(&poset) {
            let res = match (cost, new_cost) {
                (Cost::Minimum(old_min), Cost::Minimum(new_min)) => {
                    Cost::Minimum(new_min.max(old_min))
                }
                (Cost::Solved(old_solved), Cost::Solved(new_solved)) => {
                    Cost::Solved(new_solved.min(old_solved))
                }
                (Cost::Solved(_), Cost::Minimum(_)) => cost,
                (Cost::Minimum(_), Cost::Solved(_)) => new_cost,
            };

            let replaced = self.cache.insert(poset, res);
            if replaced {
                self.analytics.record_replace();
            }
        } else {
            let replaced = self.cache.insert(poset, new_cost);
            if replaced {
                self.analytics.record_replace();
            }
        }
    }

    fn heuristic_strategy_dir(&self) -> String {
        format!("logs/heuristic/strategy_{}", self.heuristic_strategy)
    }

    fn heuristic_tracked_dir(&self) -> String {
        format!("{}/tracked", self.heuristic_strategy_dir())
    }

    fn heuristic_run_dir(&self) -> String {
        format!(
            "{}/runs/n_{}_i_{}",
            self.heuristic_tracked_dir(),
            self.n,
            self.i
        )
    }

    fn ensure_dir(path: &str) {
        std::fs::DirBuilder::new()
            .recursive(true)
            .create(path)
            .unwrap();
    }

    fn print_heuristic_mode_banner(&self, fast: bool) {
        if fast {
            println!(
                "Running Fast Heuristic Search with Strategy {}",
                self.heuristic_strategy
            );
        } else {
            println!(
                "Running Heuristic Search with Strategy {}",
                self.heuristic_strategy
            );
        }

        if !matches!(self.weight_function, WeightFunction::None) {
            println!("Heuristic path mode ignores weight-function pruning.");
        }

        match self.heuristic_strategy {
            0 => println!("(Minimizing Transitive Closure  (Product))"),
            1 => println!("(Minimizing Hasse Edge (Product))"),
            2 => println!("(Minimizing Total Relations)"),
            3 => println!("(Maximizing Candidates for i-th Smallest)"),
            4 => println!("(Maximizing Compatible Rank Prefixes)"),
            5 => println!("(Total downset-sum)"),
            6 => println!("(Always i < j)"),
            _ => println!("(Default/Unknown Strategy)"),
        }
    }

    pub fn search(&mut self) -> u8 {
        const PAIR_WISE_OPTIMIZATION: bool = false;

        let start = Instant::now();

        let min = LOWER_BOUNDS[self.n as usize][self.i as usize];
        let max = UPPER_BOUNDS[self.n as usize][self.i as usize];

        let mut result = max as u8;

        for current in min.. {
            let backward_search_state = Arc::new(RwLock::new((HashMap::new(), -1)));
            let interrupt = Arc::new(AtomicBool::new(false));
            let handle = if self.use_bidirectional_search {
                let n_local = self.n;
                let i_local = self.i;
                let interrupt_local = interrupt.clone();
                let backward_search_state_local = backward_search_state.clone();
                Some(thread::spawn(move || {
                    start_search_backward(
                        &interrupt_local,
                        Some(&backward_search_state_local),
                        n_local,
                        i_local,
                        current,
                    );
                }))
            } else {
                None
            };

            let mut poset = FreePoset::new(self.n, self.i);
            let mut comparisons_done = 0u8;
            if PAIR_WISE_OPTIMIZATION {
                println!("Attention: searching with pairwise-optimisation");
                for k in (0..self.n - 1).step_by(2) {
                    comparisons_done += 1;
                    poset.add_and_close(k, k + 1);
                }
            }

            let current = current as u8 - comparisons_done;
            self.current_max = current;
            self.analytics.set_max_depth(current / 2);

            let search_result =
                self.search_rec(&backward_search_state, poset.canonified(), current, 0);

            if let Some(handle) = handle {
                interrupt.store(true, Ordering::Relaxed);
                handle.join().unwrap();
            }

            result = match search_result {
                Cost::Solved(solved) => solved + comparisons_done,
                Cost::Minimum(min) => {
                    self.analytics.multiprogress.clear().unwrap();
                    println!(
                        "n: {}, i: {} needs at least {} comparisons",
                        self.n,
                        self.i,
                        min + comparisons_done
                    );
                    println!("{}", format_duration(start));

                    continue;
                }
            };
            break;
        }

        self.analytics.complete_all();

        // Print the found solution
        println!();
        println!(
            "Congratulations. A solution was found!\n\nn: {}, i: {}",
            self.n, self.i
        );
        println!("Comparisons: {result}");
        println!();

        self.print_cache();
        println!("{}", format_duration(start));
        println!();

        result
    }

    #[allow(clippy::too_many_lines)]
    fn search_rec(
        &mut self,
        backward_search_state: &Arc<RwLock<(HashMap<BackwardsPoset, u8>, i8)>>,
        poset: PseudoCanonifiedPoset,
        max_comparisons: u8,
        depth: u8,
    ) -> Cost {
        if poset.n() == 1 {
            return Cost::Solved(0);
        }

        if max_comparisons == 0 {
            return Cost::Minimum(1);
        }

        if let Some(cost) = self.search_cache(&poset) {
            match cost {
                Cost::Solved(_) => {
                    return cost;
                }
                Cost::Minimum(min) => {
                    if min > max_comparisons {
                        return cost;
                    }
                }
            }
        }

        if self.use_bidirectional_search {
            let read_lock = backward_search_state
                .read()
                .expect("cache shouldn't be poisoned");
            if max_comparisons as i8 + 1 <= read_lock.1 {
                return if let Some(&value) = read_lock.0.get(&poset.to_backward()) {
                    Cost::Solved(value)
                } else {
                    Cost::Minimum(max_comparisons + 1)
                };
            }
        }

        if let Some(false) =
            self.estimate_solvable(backward_search_state, poset, max_comparisons, depth)
        {
            let result = Cost::Minimum(max_comparisons + 1);

            self.insert_cache(poset, result);

            return result;
        }

        let pairs = poset.get_comparison_pairs();
        let n_pairs = pairs.len() as u64;

        self.analytics.inc_length(depth, n_pairs);

        // search all comparisons
        let mut best_comparison = (0, 0);
        let mut current_best = max_comparisons + 1;
        for (first, second, i, j, _first_is_i_less_j) in pairs {
            self.analytics.update_stats(
                depth,
                self.current_max,
                self.cache.len(),
                self.cache.max_entries(),
            );

            // search the first case of the comparison
            let first_result =
                self.search_rec(backward_search_state, first, current_best - 2, depth + 1);

            if !first_result.is_solved() || first_result.value() > current_best - 2 {
                self.analytics.inc(depth, 1);
                continue;
            }

            // search the second case of the comparison
            let second_result =
                self.search_rec(backward_search_state, second, current_best - 2, depth + 1);

            if !second_result.is_solved() || second_result.value() > current_best - 2 {
                self.analytics.inc(depth, 1);
                continue;
            }

            // take the max of the branches of the comparisons
            // if the current pair maximum was worse, the
            // continues above never let this be reached
            best_comparison = (i, j);

            current_best = first_result.value().max(second_result.value()) + 1;

            self.analytics.inc(depth, 1);
        }

        let result = if current_best <= max_comparisons {
            self.comparisons.insert(poset, best_comparison);
            Cost::Solved(current_best)
        } else {
            Cost::Minimum(max_comparisons + 1)
        };

        self.analytics.inc_complete(depth, n_pairs);

        self.analytics.record_poset();

        self.insert_cache(poset, result);

        result
    }

    fn estimate_solvable(
        &mut self,
        backward_search_state: &Arc<RwLock<(HashMap<BackwardsPoset, u8>, i8)>>,
        poset: PseudoCanonifiedPoset,
        max_comparisons: u8,
        depth: u8,
    ) -> Option<bool> {
        match self.weight_function {
            WeightFunction::CompatibleSolutions => {
                let compatible_posets = poset.num_compatible_posets();
                if compatible_posets == 0
                    || (max_comparisons as u32) < (compatible_posets - 1).ilog2() + 1
                {
                    return Some(false);
                }
            }
            WeightFunction::Weight0 => {
                let weight = poset.weight0();
                // ilog2 is rounded down -> (weight - 1).ilog2() + 1 is rounded up
                if weight <= 1 || (max_comparisons as u32) <= (weight - 1).ilog2() {
                    return Some(false);
                }
            }
            WeightFunction::Weight => {
                let scale = (1..=self.n).map(|k| k as u128).product();
                let weight = poset.weight(depth as usize + max_comparisons as usize, scale);
                if weight <= scale || (max_comparisons as u32) <= ((weight / scale) - 1).ilog2() {
                    return Some(false);
                }
            }
            WeightFunction::None => {}
        }

        let (less, greater) = poset.calculate_relations();

        let mut best = (0, 0);
        let mut best_count = 0;

        for i in 0..poset.n() {
            if !(less[i as usize] == 0 && greater[i as usize] >= 2) {
                continue;
            }

            for j in i..poset.n() {
                if !(greater[j as usize] == 0 && less[j as usize] >= 2) || poset.has_order(i, j) {
                    continue;
                }

                let count = greater[i as usize] + less[j as usize];

                if count > best_count {
                    best = (i, j);
                    best_count = count;
                }
            }
        }

        if best_count > 0 {
            let cost = self.search_rec(
                backward_search_state,
                poset.with_less(best.0, best.1),
                max_comparisons,
                depth + 1,
            );
            match cost {
                Cost::Solved(solved) => {
                    return Some(solved <= max_comparisons);
                }
                Cost::Minimum(_) => {
                    return Some(false);
                }
            }
        }

        None
    }

    pub fn search_heuristic(&mut self) -> u8 {
        let start = Instant::now();

        self.print_heuristic_mode_banner(false);

        let min = LOWER_BOUNDS[self.n as usize][self.i as usize];
        let max = UPPER_BOUNDS[self.n as usize][self.i as usize];

        let mut result = max as u8;
        let mut final_poset = None;

        for current in min.. {
            let poset = FreePoset::new(self.n, self.i);
            let current = current as u8;
            self.current_max = current;
            self.analytics.set_max_depth(current / 2);

            let (canonified, mapping) = poset.canonified_with_mapping();
            let (search_result, result_poset) =
                self.search_heuristic_rec(canonified, poset, mapping, current, 0);

            result = match search_result {
                Cost::Solved(solved) => {
                    final_poset = Some(result_poset);
                    solved
                }
                Cost::Minimum(min) => {
                    self.analytics.multiprogress.clear().unwrap();
                    println!(
                        "n: {}, i: {} heuristic path not found within {} comparisons",
                        self.n, self.i, min
                    );
                    println!("{}", format_duration(start));

                    continue;
                }
            };
            break;
        }

        self.analytics.complete_all();

        println!();
        println!("Heuristic path found!\n\nn: {}, i: {}", self.n, self.i);
        println!("Path comparisons: {result}");
        println!();

        let run_dir = self.heuristic_run_dir();
        Self::ensure_dir(&run_dir);

        if let Some(poset) = final_poset {
            let dot_path = format!("{run_dir}/final_poset.dot");
            std::fs::write(&dot_path, poset.to_dot()).unwrap();
            println!("Exported final poset to: {dot_path}");
        }

        self.print_cache();
        println!("{}", format_duration(start));
        println!();

        result
    }

    fn search_heuristic_rec(
        &mut self,
        poset: PseudoCanonifiedPoset,
        full_poset: FreePoset,
        mapping: Vec<u8>,
        max_comparisons: u8,
        depth: u8,
    ) -> (Cost, FreePoset) {
        if poset.n() == 1 {
            return (Cost::Solved(0), full_poset);
        }

        if max_comparisons == 0 {
            return (Cost::Minimum(1), full_poset);
        }

        let pairs = poset.get_comparison_pairs();
        let n_pairs = pairs.len() as u64;

        self.analytics.inc_length(depth, n_pairs);

        let mut current_best = max_comparisons + 1;
        let mut best_comparison = (0, 0);
        let mut best_full_poset = full_poset;

        for &(first, second, i, j, first_is_i_less_j) in &pairs {
            self.analytics.update_stats(
                depth,
                self.current_max,
                self.cache.len(),
                self.cache.max_entries(),
            );

            let choose_i_less_j = self.heuristic_decision(
                &poset,
                first,
                second,
                i,
                j,
                first_is_i_less_j,
                self.heuristic_strategy,
            );

            let orig_i = mapping[i as usize];
            let orig_j = mapping[j as usize];

            let mut next_full_poset = full_poset;
            if choose_i_less_j {
                next_full_poset.add_and_close(orig_i, orig_j);
            } else {
                next_full_poset.add_and_close(orig_j, orig_i);
            }

            let (next_canonified, next_mapping) = next_full_poset.canonified_with_mapping();
            let child_budget = current_best - 2;

            let (result, result_poset) = self.search_heuristic_rec(
                next_canonified,
                next_full_poset,
                next_mapping,
                child_budget,
                depth + 1,
            );

            if !result.is_solved() || result.value() > child_budget {
                self.analytics.inc(depth, 1);
                continue;
            }

            best_comparison = (i, j);
            current_best = result.value() + 1;
            best_full_poset = result_poset;
            self.analytics.inc(depth, 1);

            if current_best <= 1 {
                break;
            }
        }

        let result = if current_best <= max_comparisons {
            self.comparisons.insert(poset, best_comparison);
            Cost::Solved(current_best)
        } else {
            Cost::Minimum(max_comparisons + 1)
        };

        self.analytics.inc_complete(depth, n_pairs);
        self.analytics.record_poset();
        self.insert_cache(poset, result);

        (result, best_full_poset)
    }

    /// Fast heuristic search with cache. No history tracking.
    pub fn search_heuristic_fast(&mut self) -> u8 {
        let start = Instant::now();

        self.print_heuristic_mode_banner(true);

        let min = LOWER_BOUNDS[self.n as usize][self.i as usize];
        let max = UPPER_BOUNDS[self.n as usize][self.i as usize];

        let mut result = max as u8;

        for current in min.. {
            let poset = FreePoset::new(self.n, self.i);
            let current = current as u8;
            self.current_max = current;
            self.analytics.set_max_depth(current / 2);

            let search_result = self.search_heuristic_fast_rec(poset.canonified(), current, 0);

            result = match search_result {
                Cost::Solved(solved) => solved,
                Cost::Minimum(min) => {
                    self.analytics.multiprogress.clear().unwrap();
                    println!(
                        "n: {}, i: {} heuristic path not found within {} comparisons (fast)",
                        self.n, self.i, min
                    );
                    println!("{}", format_duration(start));

                    continue;
                }
            };
            break;
        }

        self.analytics.complete_all();

        println!();
        println!("Fast heuristic path found!\n\nn: {}, i: {}", self.n, self.i);
        println!("Path comparisons: {result}");
        println!();

        self.print_cache();
        println!("{}", format_duration(start));
        println!();

        result
    }

    fn search_heuristic_fast_rec(
        &mut self,
        poset: PseudoCanonifiedPoset,
        max_comparisons: u8,
        depth: u8,
    ) -> Cost {
        if poset.n() == 1 {
            return Cost::Solved(0);
        }

        if max_comparisons == 0 {
            return Cost::Minimum(1);
        }

        if let Some(cost) = self.search_cache(&poset) {
            match cost {
                Cost::Solved(_) => return cost,
                Cost::Minimum(min) if min > max_comparisons => return cost,
                Cost::Minimum(_) => {}
            }
        }

        let pairs = poset.get_comparison_pairs();
        let n_pairs = pairs.len() as u64;

        self.analytics.inc_length(depth, n_pairs);

        let mut current_best = max_comparisons + 1;
        let mut best_comparison = (0, 0);

        const FAST_STATS_UPDATE_STRIDE: usize = 64;
        for (idx, &(first, second, i, j, first_is_i_less_j)) in pairs.iter().enumerate() {
            if idx % FAST_STATS_UPDATE_STRIDE == 0 {
                self.analytics.update_stats(
                    depth,
                    self.current_max,
                    self.cache.len(),
                    self.cache.max_entries(),
                );
            }

            let choose_i_less_j = self.heuristic_decision(
                &poset,
                first,
                second,
                i,
                j,
                first_is_i_less_j,
                self.heuristic_strategy,
            );

            let chosen_poset = if choose_i_less_j == first_is_i_less_j {
                first
            } else {
                second
            };

            let child_budget = current_best - 2;
            if let Some(cached) = self.peek_cache(&chosen_poset) {
                match cached {
                    Cost::Solved(solved) => {
                        if solved <= child_budget {
                            best_comparison = (i, j);
                            current_best = solved + 1;
                        }
                        self.analytics.inc(depth, 1);
                        if current_best <= 1 {
                            break;
                        }
                        continue;
                    }
                    Cost::Minimum(min) if min > child_budget => {
                        self.analytics.inc(depth, 1);
                        continue;
                    }
                    _ => {}
                }
            }

            let result = self.search_heuristic_fast_rec(chosen_poset, child_budget, depth + 1);

            if !result.is_solved() || result.value() > child_budget {
                self.analytics.inc(depth, 1);
                continue;
            }

            best_comparison = (i, j);
            current_best = result.value() + 1;
            self.analytics.inc(depth, 1);

            if current_best <= 1 {
                break;
            }
        }

        let result = if current_best <= max_comparisons {
            self.comparisons.insert(poset, best_comparison);
            Cost::Solved(current_best)
        } else {
            Cost::Minimum(max_comparisons + 1)
        };

        self.analytics.inc_complete(depth, n_pairs);
        self.analytics.record_poset();
        self.insert_cache(poset, result);

        result
    }

    /// Returns true for i < j, false for j < i
    fn heuristic_decision(
        &self,
        parent: &PseudoCanonifiedPoset,
        first: PseudoCanonifiedPoset,
        second: PseudoCanonifiedPoset,
        i: u8,
        j: u8,
        first_is_i_less_j: bool,
        strategy: u8,
    ) -> bool {
        match strategy {
            0 => self.heuristic_0_transitive_closure(parent, i, j),
            1 => self.heuristic_1_hasse_edges(parent, i, j),
            2 => self.heuristic_2_total_relations(first, second, first_is_i_less_j),
            3 => self.heuristic_maximize_candidates(&first, &second, first_is_i_less_j),
            4 => {
                self.heuristic_maximize_compatible_rank_prefixes(&first, &second, first_is_i_less_j)
            }
            5 => self.heuristic_5_minimize_delta_downset_sum(parent, i, j),
            6 => self.heuristic_6_fixed_i_less_j(),
            _ => true,
        }
    }

    /// Fixed baseline: always decide i < j.
    #[inline]
    fn heuristic_6_fixed_i_less_j(&self) -> bool {
        true
    }

    fn heuristic_0_transitive_closure(&self, parent: &PseudoCanonifiedPoset, i: u8, j: u8) -> bool {
        let upset_i = parent.get_all_greater_than(i).len();
        let downset_j = parent.get_all_less_than(j).len();
        let downset_i = parent.get_all_less_than(i).len();
        let upset_j = parent.get_all_greater_than(j).len();

        let impact_i_less_j = downset_i * upset_j;
        let impact_j_less_i = downset_j * upset_i;

        impact_i_less_j <= impact_j_less_i
    }

    fn count_hasse_upset_size(&self, poset: &PseudoCanonifiedPoset, i: u8) -> u32 {
        let greater_than_i = poset.get_all_greater_than(i);
        let mut count = 0;

        for j in greater_than_i {
            let j = j as u8;
            let less_than_j = poset.get_all_less_than(j);
            let intermediate = greater_than_i.intersect(less_than_j);

            if intermediate.is_empty() {
                count += 1;
            }
        }
        count
    }

    fn count_hasse_downset_size(&self, poset: &PseudoCanonifiedPoset, i: u8) -> u32 {
        let less_than_i = poset.get_all_less_than(i);
        let mut count = 0;

        for j in less_than_i {
            let j = j as u8;
            let greater_than_j = poset.get_all_greater_than(j);
            let intermediate = greater_than_j.intersect(less_than_i);

            if intermediate.is_empty() {
                count += 1;
            }
        }
        count
    }

    fn heuristic_1_hasse_edges(&self, parent: &PseudoCanonifiedPoset, i: u8, j: u8) -> bool {
        let hasse_upset_i = self.count_hasse_upset_size(parent, i);
        let hasse_downset_j = self.count_hasse_downset_size(parent, j);
        let hasse_downset_i = self.count_hasse_downset_size(parent, i);
        let hasse_upset_j = self.count_hasse_upset_size(parent, j);

        let impact_i_less_j = hasse_downset_i * hasse_upset_j;
        let impact_j_less_i = hasse_downset_j * hasse_upset_i;

        impact_i_less_j <= impact_j_less_i
    }

    fn heuristic_2_total_relations(
        &self,
        a: PseudoCanonifiedPoset,
        b: PseudoCanonifiedPoset,
        first_is_i_less_j: bool,
    ) -> bool {
        let relations_a = self.count_total_relations(&a);
        let relations_b = self.count_total_relations(&b);
        if first_is_i_less_j {
            relations_a <= relations_b
        } else {
            relations_b <= relations_a
        }
    }

    fn heuristic_5_minimize_delta_downset_sum(
        &self,
        parent: &PseudoCanonifiedPoset,
        i: u8,
        j: u8,
    ) -> bool {
        let base = parent.to_free();
        let s_parent = self.total_downset_sum(&base);

        let mut i_less_j = base;
        i_less_j.add_and_close(i, j);
        let s_i_less_j = self.total_downset_sum(&i_less_j);

        let mut j_less_i = base;
        j_less_i.add_and_close(j, i);
        let s_j_less_i = self.total_downset_sum(&j_less_i);

        debug_assert!(s_i_less_j >= s_parent);
        debug_assert!(s_j_less_i >= s_parent);

        let delta_i_less_j = s_i_less_j - s_parent;
        let delta_j_less_i = s_j_less_i - s_parent;

        delta_i_less_j <= delta_j_less_i
    }

    fn count_total_relations(&self, poset: &PseudoCanonifiedPoset) -> u32 {
        let n = poset.n() as usize;
        let (less, greater) = poset.calculate_relations();
        let mut total = 0;
        for i in 0..n {
            total += u32::from(less[i]) + u32::from(greater[i]);
        }
        total
    }

    fn total_downset_sum<P: Poset>(&self, poset: &P) -> u32 {
        let (less, _) = poset.calculate_relations();
        less[..poset.n() as usize]
            .iter()
            .map(|value| *value as u32)
            .sum()
    }

    #[inline]
    fn comparison_outcomes<'b>(
        &self,
        first: &'b PseudoCanonifiedPoset,
        second: &'b PseudoCanonifiedPoset,
        first_is_i_less_j: bool,
    ) -> (&'b PseudoCanonifiedPoset, &'b PseudoCanonifiedPoset) {
        if first_is_i_less_j {
            (first, second)
        } else {
            (second, first)
        }
    }

    fn heuristic_maximize_candidates(
        &self,
        first: &PseudoCanonifiedPoset,
        second: &PseudoCanonifiedPoset,
        first_is_i_less_j: bool,
    ) -> bool {
        let (i_less_j, j_less_i) = self.comparison_outcomes(first, second, first_is_i_less_j);

        let candidates_i_less_j = self.count_rank_candidates(i_less_j);
        let candidates_j_less_i = self.count_rank_candidates(j_less_i);

        candidates_i_less_j >= candidates_j_less_i
    }

    fn count_rank_candidates(&self, poset: &PseudoCanonifiedPoset) -> u8 {
        let (less, greater) = poset.calculate_relations();
        self.count_candidates(&less, &greater, poset.n(), poset.i())
    }

    fn count_candidates(&self, less: &[u8], greater: &[u8], n: u8, rank: u8) -> u8 {
        let max_larger = n.saturating_sub(rank.saturating_add(1));
        let mut candidates = 0u8;

        for i in 0..n as usize {
            if less[i] <= rank && greater[i] <= max_larger {
                candidates += 1;
            }
        }

        candidates
    }

    fn heuristic_maximize_compatible_rank_prefixes(
        &self,
        first: &PseudoCanonifiedPoset,
        second: &PseudoCanonifiedPoset,
        first_is_i_less_j: bool,
    ) -> bool {
        let (i_less_j, j_less_i) = self.comparison_outcomes(first, second, first_is_i_less_j);

        let count_ilj = self.count_compatible_rank_prefixes(i_less_j);
        let count_jli = self.count_compatible_rank_prefixes(j_less_i);

        count_ilj >= count_jli
    }

    fn count_compatible_rank_prefixes(&self, poset: &PseudoCanonifiedPoset) -> u64 {
        let n = poset.n();
        let target_len = poset.i() as usize + 1;

        if target_len > n as usize {
            return 0;
        }

        let (less, _) = poset.calculate_relations();
        let greater_than: Vec<BitSet> = (0..n)
            .map(|candidate| poset.get_all_greater_than(candidate))
            .collect();
        let mut memo = HashMap::new();

        self.count_compatible_rank_prefixes_rec(
            BitSet::empty(),
            target_len,
            &less[..n as usize],
            &greater_than,
            &mut memo,
        )
    }

    fn count_compatible_rank_prefixes_rec(
        &self,
        chosen: BitSet,
        target_len: usize,
        less: &[u8],
        greater_than: &[BitSet],
        memo: &mut HashMap<BitSet, u64>,
    ) -> u64 {
        if chosen.len() == target_len {
            return 1;
        }

        if let Some(&cached) = memo.get(&chosen) {
            return cached;
        }

        let rank = chosen.len() as u8;
        let mut count = 0u64;

        for candidate in 0..less.len() {
            if chosen.contains(candidate) {
                continue;
            }

            if less[candidate] > rank {
                continue;
            }

            if !greater_than[candidate].intersect(chosen).is_empty() {
                continue;
            }

            let mut next_chosen = chosen;
            next_chosen.insert(candidate);
            count = count.saturating_add(self.count_compatible_rank_prefixes_rec(
                next_chosen,
                target_len,
                less,
                greater_than,
                memo,
            ));
        }

        memo.insert(chosen, count);
        count
    }

    pub fn print_cache(&self) {
        println!("Cache entries: {}", self.cache.len());
        println!("Cache size: {:.3} Gigabyte", self.cache.size_as_gigabyte());
        println!("Cache hits: {}", self.analytics.cache_hits());
        println!("Cache misses: {}", self.analytics.cache_misses());
        println!("Cache replaced: {}", self.analytics.cache_replaced());
        println!();
        println!("Posets searched: {}", self.analytics.total_posets());
    }
}

impl Analytics {
    fn new(max_progress_depth: u8) -> Analytics {
        let multiprogress = MultiProgress::new();

        let mut progress_bars = Vec::with_capacity(max_progress_depth as usize);
        for _ in 0..max_progress_depth {
            let pb = ProgressBar::new(0)
                .with_style(ProgressStyle::with_template("[{pos:2}/{len:2}] {msg}").unwrap());
            let pb = multiprogress.add(pb);
            progress_bars.push((pb, AtomicU64::new(0)));
        }
        Analytics {
            total_posets: 0,
            cache_hits: 0,
            cache_misses: 0,
            cache_replaced: 0,
            max_progress_depth,
            multiprogress,
            progress_bars,
        }
    }

    fn set_max_depth(&mut self, new_depth: u8) {
        if new_depth > self.max_progress_depth {
            for _ in self.max_progress_depth..new_depth {
                let pb = ProgressBar::new(0)
                    .with_style(ProgressStyle::with_template("[{pos:2}/{len:2}] {msg}").unwrap());
                let pb = self.multiprogress.add(pb);
                self.progress_bars.push((pb, AtomicU64::new(0)));
            }
        } else {
            for _ in new_depth..self.max_progress_depth {
                let (pb, _) = self.progress_bars.pop().unwrap();
                pb.finish_and_clear();
                self.multiprogress.remove(&pb);
            }
        }
        self.max_progress_depth = new_depth;
    }

    #[inline]
    fn inc_length(&self, depth: u8, count: u64) {
        if depth >= self.max_progress_depth {
            return;
        }
        self.progress_bars[depth as usize].0.inc_length(count);
        self.progress_bars[depth as usize]
            .1
            .fetch_add(count, Ordering::Relaxed);
    }

    #[inline]
    fn inc(&self, depth: u8, amount: u64) {
        if depth >= self.max_progress_depth {
            return;
        }
        self.progress_bars[depth as usize].0.inc(amount);
    }

    #[inline]
    fn inc_complete(&self, depth: u8, count: u64) {
        if depth >= self.max_progress_depth {
            return;
        }
        let (pb, len) = &self.progress_bars[depth as usize];

        pb.inc(count.wrapping_neg());
        pb.set_length(len.fetch_sub(count, Ordering::Release) - count);
    }

    #[inline]
    fn update_stats(&self, depth: u8, current_max: u8, cache_entries: usize, max_entries: usize) {
        if depth >= self.max_progress_depth {
            return;
        }

        let cache_percentage = cache_entries as f64 / max_entries as f64 * 100.0;
        self.progress_bars[0].0.set_message(format!(
            "limit: {:3} total: {:10}, cache: {:10} ({:2.2} %)",
            current_max, self.total_posets, cache_entries, cache_percentage
        ));
    }

    fn complete_all(&self) {
        for i in 0..self.max_progress_depth as usize {
            let (pb, _) = &self.progress_bars[i];
            pb.finish_and_clear();
            self.multiprogress.remove(pb);
        }
    }

    #[inline]
    fn record_hit(&mut self) {
        self.cache_hits += 1;
    }

    #[inline]
    fn record_miss(&mut self) {
        self.cache_misses += 1;
    }

    #[inline]
    fn record_replace(&mut self) {
        self.cache_replaced += 1;
    }

    #[inline]
    fn record_poset(&mut self) {
        self.total_posets += 1;
    }

    fn cache_hits(&self) -> u64 {
        self.cache_hits
    }

    fn cache_misses(&self) -> u64 {
        self.cache_misses
    }

    fn cache_replaced(&self) -> u64 {
        self.cache_replaced
    }

    fn total_posets(&self) -> u64 {
        self.total_posets
    }
}

impl Drop for Analytics {
    fn drop(&mut self) {
        self.complete_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::free_poset::FreePoset;

    fn run_heuristic(n: u8, i: u8, strategy: u8, fast: bool) -> u8 {
        let mut cache = Cache::new(1 << 20);
        let mut algorithm = HashMap::new();
        let mut search = Search::new(
            n,
            i,
            &mut cache,
            &mut algorithm,
            false,
            WeightFunction::None,
            strategy,
        );

        if fast {
            search.search_heuristic_fast()
        } else {
            search.search_heuristic()
        }
    }

    #[test]
    fn heuristic_fast_matches_tracked_heuristic_on_nontrivial_instance() {
        let n = 7;
        let i = 2;

        for strategy in 0..=6 {
            let fast = run_heuristic(n, i, strategy, true);
            let tracked = run_heuristic(n, i, strategy, false);

            assert_eq!(
                fast, tracked,
                "mismatch for n={n}, i={i}, strategy={strategy}"
            );
        }
    }

    fn legacy_count_compatible_triples(poset: &PseudoCanonifiedPoset) -> u64 {
        let n = poset.n();
        let mut count = 0u64;

        for m in 0..n {
            if !poset.get_all_less_than(m).is_empty() {
                continue;
            }

            for s in 0..n {
                if s == m {
                    continue;
                }

                if (s < m && poset.is_less(s, m)) || poset.get_all_less_than(s).len() > 1 {
                    continue;
                }

                for t in 0..n {
                    if t == m || t == s {
                        continue;
                    }

                    if (t < m && poset.is_less(t, m))
                        || (t < s && poset.is_less(t, s))
                        || poset.get_all_less_than(t).len() > 2
                    {
                        continue;
                    }

                    count += 1;
                }
            }
        }

        count
    }

    #[test]
    fn compatible_rank_prefix_count_matches_legacy_triples_for_i_2() {
        let mut cache = Cache::new(1 << 20);
        let mut algorithm = HashMap::new();
        let search = Search::new(
            6,
            2,
            &mut cache,
            &mut algorithm,
            false,
            WeightFunction::None,
            4,
        );

        let mut poset = FreePoset::new(6, 2);
        poset.add_and_close(0, 2);
        poset.add_and_close(1, 4);
        let poset = poset.canonified();

        assert_eq!(
            search.count_compatible_rank_prefixes(&poset),
            legacy_count_compatible_triples(&poset)
        );
    }

    #[test]
    fn heuristic_4_fast_matches_tracked_for_general_i() {
        let n = 6;
        let i = 1;

        let fast = run_heuristic(n, i, 4, true);
        let tracked = run_heuristic(n, i, 4, false);

        assert_eq!(fast, tracked, "mismatch for n={n}, i={i}, strategy=4");
    }
}
