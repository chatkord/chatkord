//! A closed set that lists itself: an enum declared through [`listed!`](crate::listed!) gets an `ALL` of every variant, made from
//! the same list as the enum, so a new variant is in it by construction.

/// Declares a fieldless enum as written and an `ALL` of its variants in declaration order, so a variant's place in
/// `ALL` is `variant as usize`.
#[macro_export]
macro_rules! listed {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident { $($body:tt)* }
    ) => {
        // The body goes through as written, so a diagnostic about a variant points at the variant.
        $(#[$meta])*
        $vis enum $name { $($body)* }

        $crate::listed!(@all $vis $name [$($body)*] []);
    };
    (@all $vis:vis $name:ident [#[$variant_meta:meta] $($rest:tt)*] [$($variant:ident)*]) => {
        $crate::listed!(@all $vis $name [$($rest)*] [$($variant)*]);
    };
    (@all $vis:vis $name:ident [$next:ident $(, $($rest:tt)*)?] [$($variant:ident)*]) => {
        $crate::listed!(@all $vis $name [$($($rest)*)?] [$($variant)* $next]);
    };
    (@all $vis:vis $name:ident [] [$($variant:ident)+]) => {
        impl $name {
            /// Every variant once, in declaration order: a variant's place in it is `variant as usize`.
            $vis const ALL: [$name; [$(stringify!($variant)),+].len()] = [$($name::$variant),+];
        }
    };
}

#[cfg(test)]
mod tests {
    listed! {
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
        enum Step {
            First,
            /// A variant's own attributes pass through.
            #[default]
            Second,
            Third,
        }
    }

    #[test]
    fn every_variant_is_listed_once_at_its_own_place() {
        assert_eq!(Step::ALL, [Step::First, Step::Second, Step::Third]);
        for (place, step) in Step::ALL.into_iter().enumerate() {
            assert_eq!(step as usize, place, "{step:?}");
        }
        assert_eq!(Step::default(), Step::Second);
    }
}
