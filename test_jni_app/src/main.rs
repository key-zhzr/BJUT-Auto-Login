use std::str::FromStr;
fn main() {
    let sig: jni::signature::RuntimeMethodSignature = "(Landroid/content/Context;)Ljava/lang/String;".parse().unwrap();
    let method_sig = jni::signature::MethodSignature::from(&sig);
    let class_name = jni::strings::JNIString::from("cn/edu/bjut/al/NetworkHelper");
    println!("{:?}", class_name);
}
