use crate::{
    dependency::{DependencySpec, KnownDependencies},
    environment::{Environment, XPathEnvironmentSpec, XsltEnvironmentSpec},
    ns::{XPATH_TEST_NS, XSLT_TEST_NS},
    paths::Mode,
    testcase::{Runnable, XPathTestCase, XsltTestCase},
};

pub(crate) trait Language: Sized {
    type Environment: Environment;
    type Runnable: Runnable<Self>;

    fn catalog_ns() -> &'static str;
    fn mode() -> Mode;
    fn known_dependencies() -> KnownDependencies;
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct XPathLanguage {}

impl Language for XPathLanguage {
    type Environment = XPathEnvironmentSpec;
    type Runnable = XPathTestCase;

    fn catalog_ns() -> &'static str {
        XPATH_TEST_NS
    }

    fn mode() -> Mode {
        Mode::XPath
    }

    fn known_dependencies() -> KnownDependencies {
        let specs = vec![
            DependencySpec {
                type_: "spec".to_string(),
                value: "XP20+".to_string(),
            },
            DependencySpec {
                type_: "spec".to_string(),
                value: "XP30+".to_string(),
            },
            DependencySpec {
                type_: "spec".to_string(),
                value: "XP31+".to_string(),
            },
            DependencySpec {
                type_: "feature".to_string(),
                value: "higherOrderFunctions".to_string(),
            },
            DependencySpec {
                type_: "xml-version".to_string(),
                value: "1.0".to_string(),
            },
            DependencySpec {
                type_: "xsd-version".to_string(),
                value: "1.1".to_string(),
            },
            DependencySpec {
                type_: "unicode-version".to_string(),
                value: "16.0".to_string(),
            },
        ];
        KnownDependencies::new(&specs)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct XsltLanguage;

impl Language for XsltLanguage {
    type Environment = XsltEnvironmentSpec;
    type Runnable = XsltTestCase;

    fn catalog_ns() -> &'static str {
        XSLT_TEST_NS
    }

    fn mode() -> Mode {
        Mode::Xslt
    }

    fn known_dependencies() -> KnownDependencies {
        let specs = vec![
            DependencySpec {
                type_: "spec".to_string(),
                value: "XSLT10+".to_string(),
            },
            DependencySpec {
                type_: "spec".to_string(),
                value: "XSLT20+".to_string(),
            },
            DependencySpec {
                type_: "spec".to_string(),
                value: "XSLT30+".to_string(),
            },
            DependencySpec {
                type_: "feature".to_string(),
                value: "higherOrderFunctions".to_string(),
            },
            DependencySpec {
                type_: "feature".to_string(),
                value: "higher_order_functions".to_string(),
            },
            DependencySpec {
                type_: "feature".to_string(),
                value: "dynamic_evaluation".to_string(),
            },
            DependencySpec {
                type_: "feature".to_string(),
                value: "XPath_3.1".to_string(),
            },
            DependencySpec {
                type_: "feature".to_string(),
                value: "serialization".to_string(),
            },
            DependencySpec {
                type_: "feature".to_string(),
                value: "namespace_axis".to_string(),
            },
            DependencySpec {
                type_: "unicode-version".to_string(),
                value: "16.0".to_string(),
            },
        ];
        KnownDependencies::new(&specs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dependency::{Dependencies, Dependency};

    fn xslt_feature_dependency(value: &str) -> Dependencies {
        Dependencies::new(vec![Dependency {
            spec: DependencySpec {
                type_: "feature".to_string(),
                value: value.to_string(),
            },
            satisfied: true,
        }])
    }

    #[test]
    fn test_xslt_known_dependencies_support_dynamic_evaluation() {
        assert!(xslt_feature_dependency("dynamic_evaluation")
            .is_supported(&XsltLanguage::known_dependencies()));
    }

    #[test]
    fn test_xslt_known_dependencies_support_higher_order_functions_vendor_spelling() {
        assert!(xslt_feature_dependency("higher_order_functions")
            .is_supported(&XsltLanguage::known_dependencies()));
    }

    #[test]
    fn test_xslt_known_dependencies_support_xpath_31_feature_flag() {
        assert!(
            xslt_feature_dependency("XPath_3.1").is_supported(&XsltLanguage::known_dependencies())
        );
    }
}
