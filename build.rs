//! Встраивает в .exe иконку и метаданные (вкладка «Подробно» в свойствах
//! файла). Только под Windows —
//! на других платформах ресурсов такого вида просто нет.
//!
//! `cfg(windows)` в build.rs означает «сборка идёт НА Windows», а не «для
//! Windows»: build.rs всегда компилируется под хост. Ровно так же
//! разрешается `[target.'cfg(windows)'.build-dependencies]` в Cargo.toml,
//! поэтому одно согласуется с другим. Кросс-сборка с Linux на Windows
//! останется без иконки — это осознанный размен на простоту.

fn main() {
    #[cfg(windows)]
    {
        // Без этого cargo не пересоберёт ресурсы после перерисовки иконки.
        println!("cargo:rerun-if-changed=assets/mdglimpse.ico");

        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("assets/mdglimpse.ico");

        // FileVersion и ProductVersion крейт берёт из CARGO_PKG_VERSION сам,
        // поэтому руками их не задаём: иначе номер пришлось бы держать
        // в двух местах, и рано или поздно они разъедутся.
        // А вот OriginalFilename и InternalName он сам не заполняет —
        // проверено в свойствах собранного .exe, оба поля были пустыми.
        resource.set("OriginalFilename", "mdglimpse.exe");
        resource.set("InternalName", "mdglimpse");
        resource.set("FileDescription", "Просмотрщик Markdown");
        resource.set("ProductName", "MdGlimpse");
        resource.set("CompanyName", "algmironov");
        // LegalCopyright — поле для копирайта, а не для лицензии; раньше
        // здесь стояло одно только «MIT OR Apache-2.0», то есть свойства
        // .exe сообщали условия и умалчивали правообладателя.
        resource.set("LegalCopyright", "© 2026 algmironov. MIT OR Apache-2.0");

        // Здесь expect уместен, хотя в самом приложении он запрещён:
        // молча выпустить .exe без иконки хуже, чем громко не собраться,
        // а пользователя, которому можно было бы показать ошибку, ещё нет.
        resource
            .compile()
            .expect("не удалось встроить ресурсы Windows в .exe");
    }
}
