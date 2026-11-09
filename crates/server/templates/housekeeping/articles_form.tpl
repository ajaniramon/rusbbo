{# Shared article form. Expects: article (may be null), images, categories, currentDate, randomImage (create only), formAction #}
{% autoescape 'html' %}
<div class="page-head"><div></div><div class="actions"><a class="btn" href="{{ hk.path }}/articles">{{ locale.hk_volver_a_noticias_2|raw }}</a></div></div>
<div class="grid g-2">
  <div class="card">
    <div class="card-body">
      <form class="form" method="post" action="{{ formAction }}">
        <input type="hidden" name="hk_token" value="{{ hk.token }}">
        <div class="field"><label>{{ locale.hk_titulo|raw }}</label><input type="text" name="title" maxlength="64" required value="{% if article != null %}{{ article.getTitle() }}{% endif %}"></div>
        <div class="field"><label>{{ locale.hk_resumen_entradilla|raw }}</label><input type="text" name="shortstory" value="{% if article != null %}{{ article.getShortStory() }}{% endif %}"></div>
        <div class="field">
          <label>{{ locale.hk_texto_completo|raw }}</label>
          <textarea name="fullstory" id="fullstory" rows="12" oninput="hkPreview()">{% if article != null %}{{ article.getFullStory() }}{% endif %}</textarea>
          <span class="help">{{ locale.hk_admite_bbcode_b_negrita_b_i_cursiva_i_url_enlace|raw }}</span>
        </div>
        <div class="field">
          <label>{{ locale.hk_categorias|raw }}</label>
          <select id="categories" name="categories[]" multiple size="{{ categories | length }}">
            {% for c in categories %}<option value="{{ c.getIndex() }}" {% if article != null and article.hasCategory(c.getId()) %}selected{% endif %}>{{ c.getLabel() }}</option>{% endfor %}
          </select>
          <span class="help">{{ locale.hk_ctrl_cmd_clic_para_elegir_varias|raw }}</span>
        </div>
        <div class="row r-2">
          <div class="field">
            <label>{{ locale.hk_imagen_de_portada|raw }}</label>
            <select name="topstory" id="topstory" onchange="hkTopStory()">
              {% for image in images %}<option value="{{ image }}" {% if article != null and article.getTopStory() == image %}selected{% elseif article == null and image == randomImage %}selected{% endif %}>{{ image }}</option>{% endfor %}
            </select>
          </div>
          <div class="field"><label>{{ locale.hk_o_url_de_otra_imagen|raw }}</label><input type="text" name="topstoryOverride" id="topstoryOverride" oninput="hkTopStory()" value="{% if article != null %}{{ article.getTopstoryOverride() }}{% endif %}"></div>
        </div>
        <div class="preview-box"><img id="ts-preview" alt="" src="{% if article != null %}{{ article.getLiveTopStory() }}{% else %}{{ site.staticContentPath }}/c_images/Top_Story_Images/{{ randomImage }}{% endif %}"></div>
        <div class="row r-2">
          <div class="field"><label>{{ locale.hk_imagen_dentro_del_articulo_opcional|raw }}</label><input type="text" name="articleimage" value="{% if article != null %}{{ article.getArticleImage() }}{% endif %}"></div>
          <div class="field"><label>{{ locale.hk_firmar_como_opcional|raw }}</label><input type="text" name="authorOverride" value="{% if article != null %}{{ article.getAuthorOverride() }}{% endif %}" placeholder="{{ locale.hk_por_defecto_tu_nombre|raw }}"></div>
        </div>
        <div class="row r-2">
          <div class="field"><label>{{ locale.hk_fecha_de_publicacion|raw }}</label><input type="datetime-local" name="datePublished" value="{{ currentDate }}"></div>
          <div class="field" style="justify-content:flex-end; gap:8px">
            <label class="check"><input type="checkbox" name="published" value="true" {% if article == null or article.isPublished() %}checked{% endif %}> {{ locale.hk_publicada|raw }}</label>
            <label class="check"><input type="checkbox" name="futurePublished" value="true" {% if article != null and article.isFuturePublished() %}checked{% endif %}> {{ locale.hk_programada_publicar_en_esa_fecha|raw }}</label>
          </div>
        </div>
        <div class="form-actions"><button class="btn primary" type="submit">{% if article != null %}{{ locale.hk_guardar_noticia|raw }}{% else %}{{ locale.hk_publicar_noticia|raw }}{% endif %}</button></div>
      </form>
    </div>
  </div>
  <div class="card">
    <div class="card-head"><h2>{{ locale.hk_vista_previa|raw }}</h2></div>
    <div class="card-body" id="news-preview">{% if article != null %}{{ article.getEscapedStory() | raw }}{% else %}<span class="muted">{{ locale.hk_escribe_el_texto_para_ver_como_queda|raw }}</span>{% endif %}</div>
  </div>
</div>
{% endautoescape %}
<script>
var hkStatic = "{{ site.staticContentPath | escape('js') }}/c_images/Top_Story_Images/";
function hkTopStory() {
  var custom = document.getElementById('topstoryOverride').value;
  var sel = document.getElementById('topstory');
  document.getElementById('ts-preview').src = custom ? custom : hkStatic + sel.value;
}
var hkTimer = null;
function hkPreview() {
  clearTimeout(hkTimer);
  hkTimer = setTimeout(function () {
    var body = document.getElementById('fullstory').value;
    var xhr = new XMLHttpRequest();
    xhr.open('POST', '/habblet/ajax/preview_news_article');
    xhr.setRequestHeader('Content-Type', 'application/x-www-form-urlencoded');
    xhr.onreadystatechange = function () {
      if (xhr.readyState === 4) { document.getElementById('news-preview').innerHTML = xhr.responseText; }
    };
    xhr.send('body=' + encodeURIComponent(body));
  }, 300);
}
</script>
